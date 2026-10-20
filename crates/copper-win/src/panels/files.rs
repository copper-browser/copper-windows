//! Downloads and the Windows shell: a file's own icon (as Explorer draws
//! it), opening a file or folder, and showing a file selected in Explorer
//! (the Mac's "Show in Finder").
//!
//! (`sys/shell.rs` is the home of shell hand-offs; `reveal` and `icon`
//! belong there and live here until they move.)

use crate::picture::Picture;
use crate::platform::Icon;
use crate::sys::wide;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{
    SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
    ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetIconInfo, HICON, ICONINFO, SW_SHOWNORMAL,
};
use windows::core::{PCWSTR, w};

thread_local! {
    /// Icons by extension (or by path, for files that carry their own).
    static ICONS: RefCell<HashMap<String, Option<Icon>>> = RefCell::default();
    /// Icons being fetched.
    static ASKED: RefCell<std::collections::HashSet<String>> = RefCell::default();
}

/// Kinds of file whose icon is their own, not their type's.
const OWN_ICON: [&str; 5] = ["exe", "lnk", "ico", "url", "msi"];

/// Open a file (or folder) with what Windows opens it with.
pub fn open(path: &str) {
    if let Err(e) = crate::sys::shell::open(path) {
        tracing::warn!("{e}");
    }
}

/// Show `path` selected in an Explorer window (`explorer /select,`).
pub fn reveal(path: &str) {
    let args = wide(&format!("/select,\"{path}\""));
    // SAFETY: NUL-terminated strings that outlive the call.
    let r = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("explorer.exe"),
            PCWSTR(args.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if r.0 as isize <= 32 {
        tracing::warn!("couldn't show {path} in Explorer (error {})", r.0 as isize);
    }
}

/// Where `name`'s icon is cached, what to ask the shell for, and whether
/// by its type alone.
fn source(path: Option<&str>, name: &str, exists: bool) -> (String, String, bool) {
    let ext = Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let own = exists && OWN_ICON.contains(&ext.as_str());
    match (own, path) {
        (true, Some(p)) => (p.to_lowercase(), p.to_string(), false),
        _ => (format!(".{ext}"), format!("file.{ext}"), true),
    }
}

/// The icon Explorer shows for `path` (named `name`) if it is already at
/// hand: `Some(None)` when Windows has none, `None` when not yet asked
/// (ask with [`warm`]; the shell is too slow to ask while drawing).
pub fn cached(path: Option<&str>, name: &str, exists: bool) -> Option<Option<Icon>> {
    let (key, _, _) = source(path, name, exists);
    ICONS.with(|c| c.borrow().get(&key).cloned())
}

/// Fetch the icons not yet at hand on a thread of their own, then `done`
/// (on this thread). Files already being fetched are not asked twice.
pub fn warm(files: Vec<(Option<String>, String, bool)>, done: impl FnOnce() + 'static) {
    let mut ask: Vec<(String, String, bool)> = Vec::new();
    for (path, name, exists) in &files {
        let (key, target, by_type) = source(path.as_deref(), name, *exists);
        let known = ICONS.with(|c| c.borrow().contains_key(&key));
        let asked = ASKED.with(|a| !a.borrow_mut().insert(key.clone()));
        if !known && !asked && !ask.iter().any(|(k, _, _)| *k == key) {
            ask.push((key, target, by_type));
        }
    }
    if ask.is_empty() {
        return;
    }
    copper_win_rt::spawn_local(async move {
        let got = super::off_thread(move || {
            // SAFETY: COM for this thread alone, as the shell asks.
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            ask.into_iter()
                .map(|(key, target, by_type)| (key, shell_icon(&target, by_type)))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        for (key, picture) in got {
            ASKED.with(|a| a.borrow_mut().remove(&key));
            ICONS.with(|c| c.borrow_mut().insert(key, picture.map(Rc::new)));
        }
        done();
    });
}

/// Ask the shell for `target`'s large icon (`by_type`: from its extension
/// alone, without touching the disk).
fn shell_icon(target: &str, by_type: bool) -> Option<Picture> {
    let t = wide(target);
    let mut info = SHFILEINFOW::default();
    let mut flags = SHGFI_ICON | SHGFI_LARGEICON;
    let mut attrs = FILE_FLAGS_AND_ATTRIBUTES(0);
    if by_type {
        flags |= SHGFI_USEFILEATTRIBUTES;
        attrs = FILE_ATTRIBUTE_NORMAL;
    }
    // SAFETY: a NUL-terminated path and an out-struct of the right size.
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(t.as_ptr()),
            attrs,
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            flags,
        )
    };
    if ok == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let picture = pixels(info.hIcon);
    // SAFETY: the icon SHGetFileInfo made for us.
    unsafe {
        let _ = DestroyIcon(info.hIcon);
    }
    picture
}

/// An icon's pixels as premultiplied BGRA.
fn pixels(icon: HICON) -> Option<Picture> {
    let mut ii = ICONINFO::default();
    // SAFETY: a live icon; the bitmaps it hands out are ours to delete.
    unsafe { GetIconInfo(icon, &mut ii).ok()? };
    let color = bitmap_bgra(ii.hbmColor);
    let mask = bitmap_bgra(ii.hbmMask);
    // SAFETY: the bitmaps GetIconInfo created.
    unsafe {
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        }
    }
    let (w, h, mut bgra) = color?;
    let has_alpha = bgra.chunks(4).any(|p| p[3] != 0);
    for (i, p) in bgra.chunks_mut(4).enumerate() {
        let a = if has_alpha {
            p[3]
        } else {
            // An old icon: the mask says what shows (black = shown).
            match &mask {
                Some((mw, _, m)) if *mw == w => {
                    if m.get(i * 4).copied().unwrap_or(0) == 0 {
                        255
                    } else {
                        0
                    }
                }
                _ => 255,
            }
        };
        let pm = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
        // Windows icons with alpha are straight, not premultiplied.
        p[0] = pm(p[0]);
        p[1] = pm(p[1]);
        p[2] = pm(p[2]);
        p[3] = a;
    }
    Some(Picture { w, h, bgra })
}

/// A bitmap's pixels, top-down 32-bit BGRA.
fn bitmap_bgra(bmp: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    if bmp.is_invalid() {
        return None;
    }
    let mut b = BITMAP::default();
    // SAFETY: reading a live bitmap's header into a struct of its size.
    let n = unsafe {
        GetObjectW(
            HGDIOBJ(bmp.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut b as *mut BITMAP as *mut _),
        )
    };
    if n == 0 || b.bmWidth <= 0 || b.bmHeight <= 0 || b.bmWidth > 256 || b.bmHeight > 256 {
        return None;
    }
    let (w, h) = (b.bmWidth as u32, b.bmHeight as u32);
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            // Negative: top-down rows.
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    // SAFETY: a memory DC for the copy, a buffer of the size asked for.
    let lines = unsafe {
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(
            dc,
            bmp,
            0,
            h,
            Some(buf.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        lines
    };
    (lines > 0).then_some((w, h, buf))
}
