//! A resting chip's emoji, greyed (`SpaceChip`'s `.saturation(0)`).
//!
//! The toolkit draws every emoji in full colour, so the grey one is a
//! picture: the emoji drawn once with Direct2D and DirectWrite into a WIC
//! bitmap at the window's scale, its colour taken out (Rec. 709 luma, the
//! same weights Core Image's saturation uses), handed to the window as an
//! image, and kept by emoji and size. A change of scale lets every picture
//! go and draws them again at the new one.

use copper_win_ui::win::Ui;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1CreateFactory, ID2D1Factory,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapCacheOnLoad,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::core::w;

/// Picture numbers of our own, far above the favicons' (`Images` counts
/// up from 1).
const FIRST_ID: u32 = 0x4752_0000;

#[derive(Default)]
struct Cache {
    /// The scale the pictures were drawn at.
    scale: f32,
    /// By emoji and size (DIPs × 8).
    pictures: HashMap<(String, u32), u32>,
    next: u32,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// The picture of `emoji` greyed, `size` DIPs square, drawn with a font of
/// `font` DIPs; `None` if it could not be drawn (the chip then shows it in
/// colour).
pub fn grey(ui: &mut Ui, emoji: &str, size: f32, font: f32) -> Option<u32> {
    let scale = ui.scale();
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if (c.scale - scale).abs() > f32::EPSILON {
            for id in std::mem::take(&mut c.pictures).into_values() {
                ui.remove_image(id);
            }
            c.scale = scale;
        }
        let key = (emoji.to_string(), (size * 8.0).round() as u32);
        if let Some(id) = c.pictures.get(&key) {
            return Some(*id);
        }
        let px = (size * scale).round().max(1.0) as u32;
        let mut pixels = match draw(emoji, px, font * scale) {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("grey emoji {emoji:?}: {e}");
                return None;
            }
        };
        desaturate(&mut pixels);
        c.next = c.next.wrapping_add(1);
        let id = FIRST_ID + c.next;
        ui.set_image(id, pixels, px, px);
        c.pictures.insert(key, id);
        Some(id)
    })
}

/// Take the colour out of premultiplied BGRA, keeping its light and its
/// alpha (a weighted sum of premultiplied channels stays premultiplied).
pub(crate) fn desaturate(bgra: &mut [u8]) {
    let (pixels, _) = bgra.as_chunks_mut::<4>();
    for p in pixels {
        let (b, g, r) = (f32::from(p[0]), f32::from(p[1]), f32::from(p[2]));
        let y = (0.2126 * r + 0.7152 * g + 0.0722 * b)
            .round()
            .min(f32::from(p[3]));
        let y = y as u8;
        p[0] = y;
        p[1] = y;
        p[2] = y;
    }
}

/// `emoji` in colour, centred in a `px` square, premultiplied BGRA.
fn draw(emoji: &str, px: u32, font_px: f32) -> windows::core::Result<Vec<u8>> {
    let text: Vec<u16> = emoji.encode_utf16().collect();
    // SAFETY: plain COM/D2D/DWrite calls on this thread with valid
    // arguments; every buffer outlives the call it is passed to.
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let bitmap =
            wic.CreateBitmap(px, px, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad)?;
        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let target = d2d.CreateWicBitmapRenderTarget(&bitmap, &props)?;
        let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let format = dw.CreateTextFormat(
            w!("Segoe UI Emoji"),
            None,
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            font_px.max(1.0),
            w!(""),
        )?;
        format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        let ink = target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.5,
                g: 0.5,
                b: 0.5,
                a: 1.0,
            },
            None,
        )?;
        let side = px as f32;
        target.BeginDraw();
        target.Clear(Some(&D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        }));
        target.DrawText(
            &text,
            &format,
            &D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: side,
                bottom: side,
            },
            &ink,
            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
            DWRITE_MEASURING_MODE_NATURAL,
        );
        target.EndDraw(None, None)?;
        let mut out = vec![0u8; (px * px * 4) as usize];
        bitmap.CopyPixels(std::ptr::null(), px * 4, &mut out)?;
        Ok(out)
    }
}
