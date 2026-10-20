//! The wake cover on Windows (Mac `Stage.swift` 22-42, 69-94): a slept
//! tab's last picture over the stage while its page is rebuilt underneath.
//!
//! The shell keeps the pictures and says when a cover goes up and comes
//! down ([`copper_shell_core::cover`]); this draws it: the page's ground
//! colour over the whole stage, the picture in its top-left corner at the
//! size it was taken (never scaled to fill, so a stage that has grown since
//! shows more page-coloured ground), cut off where the stage ends, never in
//! the way of a click. It is there the moment the tab is picked (not faded
//! in) and fades out over 0.2 s.
//!
//! One cover node lives over the stage's panes; it shows whichever tab is
//! on stage, if that tab's cover is up.

use crate::platform::Shell;
use copper_core::TabId;
use copper_shell_core::cover::{Cover, FADE};
use copper_win_ui::win::Ui;
use copper_win_ui::{Abs, Color, Motion, Node, NodeId, Prop, Role, Swatch, Token};
use std::cell::RefCell;
use std::rc::Rc;

/// The picture's number among the window's pictures (far from the
/// favicons', which count up from 1).
const IMAGE: u32 = u32::MAX - 0x00C0;

/// The cover's nodes, once made.
#[derive(Clone, Copy)]
struct Nodes {
    root: NodeId,
    picture: NodeId,
}

/// The cover the picture holds.
struct Up {
    tab: TabId,
    /// Bumped by every change, so an older fade or decode leaves a newer
    /// cover alone.
    turn: u64,
}

#[derive(Default)]
struct State {
    nodes: Option<Nodes>,
    up: Option<Up>,
    turn: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// Draw the shell's covers.
pub fn register(shell: &Shell) {
    shell.on_cover(|shell, tab, cover| match cover {
        Some(c) => put_up(shell, tab, c.clone()),
        None => take_down(shell, tab),
    });
}

/// The stage shows `tab` now (`chrome::stage`'s `show`): the cover follows.
pub fn shown(ui: &mut Ui, tab: Option<&TabId>) {
    let (nodes, on) = STATE.with(|s| {
        let s = s.borrow();
        (s.nodes, s.up.as_ref().is_some_and(|u| Some(&u.tab) == tab))
    });
    if let Some(n) = nodes {
        ui.tree.set_hidden(n.root, !on);
        if on {
            ui.tree.set_prop(n.root, Prop::Opacity, 1.0);
        }
    }
}

fn put_up(shell: &Shell, tab: &TabId, cover: Rc<Cover>) {
    let turn = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.turn += 1;
        s.up = Some(Up {
            tab: tab.clone(),
            turn: s.turn,
        });
        s.turn
    });
    let chrome = Rc::downgrade(shell.view().chrome());
    let tab = tab.clone();
    let jpeg = cover.jpeg.clone();
    copper_win_rt::spawn_local(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let worker = std::thread::Builder::new()
            .name("copper-cover".into())
            .spawn(move || {
                let _ = tx.send(decode_jpeg(&jpeg));
            });
        if worker.is_err() {
            return;
        }
        let Ok(Some((bgra, w, h))) = rx.await else {
            tracing::debug!("cover: the picture would not decode");
            return;
        };
        let Some(chrome) = chrome.upgrade() else {
            return;
        };
        let ground = cover
            .ground
            .or_else(|| copper_shell_core::cover::ground(w, h, w as usize * 4, &bgra, [2, 1, 0]));
        chrome.edit(move |ui, st| {
            if STATE.with(|s| s.borrow().up.as_ref().map(|u| u.turn)) != Some(turn) {
                return;
            }
            let n = nodes(ui, st.stage.stage);
            ui.set_image(IMAGE, bgra, w, h);
            // Drawn at the size it was taken (the pixels over the window's
            // scale when the engine did not say).
            let (sw, sh) = if cover.size.0 > 0 && cover.size.1 > 0 {
                (cover.size.0 as f32, cover.size.1 as f32)
            } else {
                let scale = ui.scale().max(0.5);
                (w as f32 / scale, h as f32 / scale)
            };
            ui.tree.edit(n.picture, |p| {
                p.style.width = copper_win_ui::Dim::Px(sw);
                p.style.height = copper_win_ui::Dim::Px(sh);
            });
            let ground = ground
                .map(|[r, g, b]| Swatch::Fixed(Color::hex(u32::from_be_bytes([0, r, g, b]))))
                .unwrap_or(Swatch::Token(Token::Ground));
            tracing::debug!(w, h, sw, sh, "cover drawn");
            ui.tree.edit(n.root, |r| r.look.fill = ground);
            ui.tree.set_prop(n.root, Prop::Opacity, 1.0);
            ui.tree.set_hidden(n.root, st.stage.shown() != Some(&tab));
        });
    });
}

fn take_down(shell: &Shell, tab: &TabId) {
    let Some(turn) = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let ours = s.up.as_ref().is_some_and(|u| &u.tab == tab);
        if !ours {
            return None;
        }
        s.turn += 1;
        let turn = s.turn;
        if let Some(u) = s.up.as_mut() {
            u.turn = turn;
        }
        Some(turn)
    }) else {
        return;
    };
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |ui, _| {
        if let Some(n) = STATE.with(|s| s.borrow().nodes) {
            ui.tree.animate(
                n.root,
                Prop::Opacity,
                0.0,
                Motion::ease_out(FADE.as_secs_f32()),
            );
        }
    });
    let weak = Rc::downgrade(&chrome);
    copper_win_rt::spawn_local(async move {
        copper_win_rt::sleep(FADE).await;
        let Some(c) = weak.upgrade() else { return };
        c.edit(move |ui, _| {
            let gone = STATE.with(|s| {
                let mut s = s.borrow_mut();
                if s.up.as_ref().map(|u| u.turn) != Some(turn) {
                    return None;
                }
                s.up = None;
                s.nodes
            });
            if let Some(n) = gone {
                ui.tree.set_hidden(n.root, true);
                ui.remove_image(IMAGE);
            }
        });
    });
}

/// The cover's nodes, made the first time: over every pane (the panes go
/// in at the stage's front) and under the two overlays the stage adds last
/// (the progress line and the hovered link's address).
fn nodes(ui: &mut Ui, stage: NodeId) -> Nodes {
    if let Some(n) = STATE.with(|s| s.borrow().nodes) {
        return n;
    }
    let at = ui.tree.children(stage).len().saturating_sub(2);
    let root = ui.tree.insert(
        stage,
        at,
        Node::stack()
            .absolute(Abs::FILL)
            .fill(Token::Ground)
            .clip()
            .layer()
            .inert()
            .hidden(true)
            .role(Role::None),
    );
    let picture = ui.tree.add(
        root,
        Node::image(IMAGE, 1.0)
            .absolute(Abs {
                left: Some(0.0),
                top: Some(0.0),
                right: None,
                bottom: None,
            })
            .role(Role::None),
    );
    let n = Nodes { root, picture };
    STATE.with(|s| s.borrow_mut().nodes = Some(n));
    n
}

/// A JPEG as premultiplied BGRA (opaque, so premultiplied is plain), with
/// WIC on this (worker) thread.
fn decode_jpeg(jpeg: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    use windows::Win32::Graphics::Imaging::{
        CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
        WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Shell::SHCreateMemStream;
    // SAFETY: COM set up and torn down on this thread; every buffer
    // outlives the call it is passed to.
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let out = (|| -> windows::core::Result<(Vec<u8>, u32, u32)> {
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = SHCreateMemStream(Some(jpeg)).ok_or_else(|| {
                windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY)
            })?;
            let decoder = wic.CreateDecoderFromStream(
                &stream,
                std::ptr::null(),
                WICDecodeMetadataCacheOnDemand,
            )?;
            let frame = decoder.GetFrame(0)?;
            let converter = wic.CreateFormatConverter()?;
            converter.Initialize(
                &frame,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )?;
            let (mut w, mut h) = (0u32, 0u32);
            converter.GetSize(&mut w, &mut h)?;
            let stride = w as usize * 4;
            let mut bgra = vec![0u8; stride * h as usize];
            converter.CopyPixels(std::ptr::null(), stride as u32, &mut bgra)?;
            Ok((bgra, w, h))
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        out.ok()
    }
}
