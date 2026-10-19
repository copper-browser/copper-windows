//! Drawing for the setup window: Direct3D 11 → DXGI composition swap chain
//! → DirectComposition visual, painted with Direct2D and DirectWrite.
//!
//! The window has no redirection bitmap; its content is one premultiplied-
//! alpha swap chain, so wherever nothing is painted the DWM backdrop (Mica)
//! shows through, with no GDI anywhere. All drawing is in DIPs: the device
//! context's DPI is the window's, so a 13-DIP line of text is 13 DIPs on any
//! monitor.

use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_CAP_STYLE_ROUND, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_OPTIONS,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
    D2D1_LINE_JOIN_ROUND, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES1,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext,
    ID2D1Factory1, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWRITE_TRIMMING,
    DWRITE_TRIMMING_GRANULARITY_WORD, DWRITE_WORD_WRAPPING_WRAP, DWriteCreateFactory,
    IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISwapChain1,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
};
use windows_core::{BOOL, HSTRING, Interface, PCWSTR, w};
use windows_numerics::Vector2;

/// The resource id of the app icon in copper.exe (copper-win's build.rs
/// embeds `assets/copper.ico` as `1 ICON`).
pub const APP_ICON_ID: u16 = 1;

/// A rectangle in DIPs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }
    pub fn offset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }
}

/// The type styles setup uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// "Copper", "Copper is ready": 22 DIP semibold, display optical size.
    Title,
    /// The one line under the title: 13 DIP.
    Body,
    /// Status under the progress bar, checkbox labels, links: 12 DIP.
    Small,
    /// Button labels: 13 DIP medium.
    Button,
}

/// Horizontal text alignment in its column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Leading,
    Center,
}

/// Text formats, made once.
struct Fonts {
    factory: IDWriteFactory,
    title: IDWriteTextFormat,
    body: IDWriteTextFormat,
    small: IDWriteTextFormat,
    button: IDWriteTextFormat,
}

/// The first of `names` the system has (Segoe UI Variable on Windows 11,
/// Segoe UI on Windows 10).
fn pick_family(collection: &IDWriteFontCollection, names: &[&'static str]) -> &'static str {
    for &name in names {
        let mut index = 0u32;
        let mut exists = BOOL(0);
        // SAFETY: valid out-pointers.
        let ok =
            unsafe { collection.FindFamilyName(&HSTRING::from(name), &mut index, &mut exists) };
        if ok.is_ok() && exists.as_bool() {
            return name;
        }
    }
    names.last().copied().unwrap_or("Segoe UI")
}

impl Fonts {
    fn new() -> anyhow::Result<Fonts> {
        // SAFETY: DirectWrite factory and format creation with valid args.
        unsafe {
            let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let mut collection = None;
            factory.GetSystemFontCollection(&mut collection, false)?;
            let collection = collection.ok_or_else(|| anyhow::anyhow!("no system fonts"))?;
            let display = pick_family(&collection, &["Segoe UI Variable Display", "Segoe UI"]);
            let text = pick_family(&collection, &["Segoe UI Variable Text", "Segoe UI"]);
            let format = |family: &str, weight: DWRITE_FONT_WEIGHT, size: f32| {
                let f = factory.CreateTextFormat(
                    &HSTRING::from(family),
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!(""),
                )?;
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR)?;
                windows_core::Result::Ok(f)
            };
            Ok(Fonts {
                title: format(display, DWRITE_FONT_WEIGHT_SEMI_BOLD, 22.0)?,
                body: format(text, DWRITE_FONT_WEIGHT_NORMAL, 13.0)?,
                small: format(text, DWRITE_FONT_WEIGHT_NORMAL, 12.0)?,
                // Medium, not semibold: the Mac's ink pill reads quiet.
                button: format(text, DWRITE_FONT_WEIGHT_MEDIUM, 13.0)?,
                factory,
            })
        }
    }

    fn format(&self, style: Style) -> &IDWriteTextFormat {
        match style {
            Style::Title => &self.title,
            Style::Body => &self.body,
            Style::Small => &self.small,
            Style::Button => &self.button,
        }
    }

    /// Lay `text` out in `width` DIPs, centred or leading.
    fn layout(
        &self,
        text: &str,
        style: Style,
        width: f32,
        center: bool,
    ) -> Option<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: valid string and format.
        unsafe {
            let layout = self
                .factory
                .CreateTextLayout(&wide, self.format(style), width.max(1.0), 1000.0)
                .ok()?;
            let align = if center {
                DWRITE_TEXT_ALIGNMENT_CENTER
            } else {
                DWRITE_TEXT_ALIGNMENT_LEADING
            };
            layout.SetTextAlignment(align).ok()?;
            Some(layout)
        }
    }
}

fn metrics(layout: &IDWriteTextLayout) -> DWRITE_TEXT_METRICS {
    let mut m = DWRITE_TEXT_METRICS::default();
    // SAFETY: valid out-pointer.
    let _ = unsafe { layout.GetMetrics(&mut m) };
    m
}

/// Everything needed to draw one frame.
pub struct Gfx {
    dc: ID2D1DeviceContext,
    swap: IDXGISwapChain1,
    // Kept alive for as long as the window shows their output.
    _factory: ID2D1Factory1,
    _device: ID3D11Device,
    _dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    target: Option<ID2D1Bitmap1>,
    brush: ID2D1SolidColorBrush,
    round: ID2D1StrokeStyle,
    fonts: Fonts,
    icon: Option<ID2D1Bitmap1>,
    size: (u32, u32),
    dpi: f32,
}

fn create_device(kind: D3D_DRIVER_TYPE) -> windows_core::Result<ID3D11Device> {
    let mut device = None;
    // SAFETY: standard D3D11 device creation; BGRA support is what D2D needs.
    unsafe {
        D3D11CreateDevice(
            None,
            kind,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(|| windows_core::Error::from_hresult(windows::Win32::Foundation::E_FAIL))
}

/// The app icon from copper.exe's resources as a D2D bitmap, at 256 px so
/// Direct2D's high-quality cubic filter does the scaling.
fn load_icon(dc: &ID2D1DeviceContext) -> Option<ID2D1Bitmap1> {
    // SAFETY: resource lookup in our own module, WIC conversion of the icon,
    // icon destroyed after the copy.
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let handle = LoadImageW(
            Some(module.into()),
            PCWSTR(APP_ICON_ID as usize as *const u16),
            IMAGE_ICON,
            256,
            256,
            LR_DEFAULTCOLOR,
        )
        .ok()?;
        let icon = HICON(handle.0);
        let result = (|| -> windows_core::Result<ID2D1Bitmap1> {
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let bitmap = wic.CreateBitmapFromHICON(icon)?;
            let converter = wic.CreateFormatConverter()?;
            converter.Initialize(
                &bitmap,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )?;
            dc.CreateBitmapFromWicBitmap(&converter, None)
        })();
        let _ = DestroyIcon(icon);
        result.ok()
    }
}

impl Gfx {
    /// Build the whole pipeline for `hwnd` at `size` physical pixels.
    pub fn new(hwnd: HWND, size: (u32, u32), dpi: f32) -> anyhow::Result<Gfx> {
        // SAFETY: the standard D3D11 → D2D → DComp bring-up; every object is
        // owned by the returned Gfx.
        unsafe {
            // A GPU if there is one; WARP (software) on a VM without one.
            let device = create_device(D3D_DRIVER_TYPE_HARDWARE)
                .or_else(|_| create_device(D3D_DRIVER_TYPE_WARP))?;
            let dxgi: IDXGIDevice = device.cast()?;

            let factory: ID2D1Factory1 = D2D1CreateFactory(
                D2D1_FACTORY_TYPE_SINGLE_THREADED,
                Some(&D2D1_FACTORY_OPTIONS::default()),
            )?;
            let d2d_device = factory.CreateDevice(&dxgi)?;
            let dc = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

            let dxgi_factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: size.0.max(1),
                Height: size.1.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swap = dxgi_factory.CreateSwapChainForComposition(&device, &desc, None)?;

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;

            let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)?;
            let round = factory
                .CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES1 {
                        startCap: D2D1_CAP_STYLE_ROUND,
                        endCap: D2D1_CAP_STYLE_ROUND,
                        lineJoin: D2D1_LINE_JOIN_ROUND,
                        ..Default::default()
                    },
                    None,
                )?
                .into();
            dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            // ClearType needs an opaque ground; over Mica it would fringe.
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let icon = load_icon(&dc);
            Ok(Gfx {
                dc,
                swap,
                _factory: factory,
                _device: device,
                _dcomp: dcomp,
                _target: target,
                _visual: visual,
                target: None,
                brush,
                round,
                fonts: Fonts::new()?,
                icon,
                size,
                dpi,
            })
        }
    }

    /// New pixel size or DPI (after WM_DPICHANGED).
    pub fn resize(&mut self, size: (u32, u32), dpi: f32) -> anyhow::Result<()> {
        self.dpi = dpi;
        if size == self.size {
            return Ok(());
        }
        self.size = size;
        // The back buffers can't resize while D2D still holds one.
        self.target = None;
        // SAFETY: no outstanding buffer references (dropped above).
        unsafe {
            self.dc.SetTarget(None);
            self.swap.ResizeBuffers(
                0,
                size.0.max(1),
                size.1.max(1),
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }
        Ok(())
    }

    fn target(&mut self) -> anyhow::Result<ID2D1Bitmap1> {
        if let Some(t) = &self.target {
            return Ok(t.clone());
        }
        // SAFETY: the swap chain's current back buffer, wrapped as a D2D
        // target with the window's DPI.
        let t = unsafe {
            let surface: IDXGISurface = self.swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: self.dpi,
                dpiY: self.dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            self.dc
                .CreateBitmapFromDxgiSurface(&surface, Some(&props))?
        };
        self.target = Some(t.clone());
        Ok(t)
    }

    /// Draw one frame: clear to `ground` (transparent when Mica is behind),
    /// let `paint` draw, present. An error means the device was lost; drop
    /// this Gfx and build a new one.
    pub fn frame(
        &mut self,
        ground: D2D1_COLOR_F,
        paint: impl FnOnce(&Painter<'_>),
    ) -> anyhow::Result<()> {
        let target = self.target()?;
        // SAFETY: draw calls between BeginDraw/EndDraw on our own context.
        unsafe {
            self.dc.SetTarget(&target);
            self.dc.SetDpi(self.dpi, self.dpi);
            self.dc.BeginDraw();
            self.dc.Clear(Some(&ground));
            paint(&Painter { gfx: self });
            let end = self.dc.EndDraw(None, None);
            self.dc.SetTarget(None);
            end?;
            // Flip-model presents are already paced by DWM; the frame loop
            // waits on DwmFlush, so don't block twice here.
            self.swap.Present(0, DXGI_PRESENT(0)).ok()?;
        }
        // Front and back buffers swap on present; rebuild the target lazily.
        self.target = None;
        Ok(())
    }
}

/// Drawing helpers, valid inside [`Gfx::frame`].
pub struct Painter<'a> {
    gfx: &'a Gfx,
}

impl Painter<'_> {
    /// The graphics this painter draws with (for measuring text).
    pub fn gfx(&self) -> &Gfx {
        self.gfx
    }

    fn brush(&self, color: D2D1_COLOR_F) -> &ID2D1SolidColorBrush {
        // SAFETY: our brush.
        unsafe { self.gfx.brush.SetColor(&color) };
        &self.gfx.brush
    }

    pub fn fill_round(&self, r: Rect, radius: f32, color: D2D1_COLOR_F) {
        if color.a <= 0.0 {
            return;
        }
        let rr = D2D1_ROUNDED_RECT {
            rect: r.d2d(),
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: inside BeginDraw.
        unsafe { self.gfx.dc.FillRoundedRectangle(&rr, self.brush(color)) }
    }

    pub fn stroke_round(&self, r: Rect, radius: f32, width: f32, color: D2D1_COLOR_F) {
        if color.a <= 0.0 {
            return;
        }
        // Strokes straddle the edge: pull in by half so the outside edge is r.
        let r = r.inset(width / 2.0);
        let rr = D2D1_ROUNDED_RECT {
            rect: r.d2d(),
            radiusX: (radius - width / 2.0).max(0.0),
            radiusY: (radius - width / 2.0).max(0.0),
        };
        // SAFETY: inside BeginDraw.
        unsafe {
            self.gfx
                .dc
                .DrawRoundedRectangle(&rr, self.brush(color), width, None)
        }
    }

    pub fn fill_rect(&self, r: Rect, color: D2D1_COLOR_F) {
        if color.a <= 0.0 {
            return;
        }
        // SAFETY: inside BeginDraw.
        unsafe { self.gfx.dc.FillRectangle(&r.d2d(), self.brush(color)) }
    }

    /// A round-capped line, for glyphs (×, ✓, chevrons) drawn as strokes.
    pub fn line(&self, a: (f32, f32), b: (f32, f32), width: f32, color: D2D1_COLOR_F) {
        if color.a <= 0.0 {
            return;
        }
        // SAFETY: inside BeginDraw.
        unsafe {
            self.gfx.dc.DrawLine(
                Vector2 { X: a.0, Y: a.1 },
                Vector2 { X: b.0, Y: b.1 },
                self.brush(color),
                width,
                &self.gfx.round,
            )
        }
    }

    /// Draw `text` from `(x, y)` in a column `w` wide, wrapping; returns
    /// its height.
    pub fn text(
        &self,
        text: &str,
        style: Style,
        (x, y): (f32, f32),
        w: f32,
        align: Align,
        color: D2D1_COLOR_F,
    ) -> f32 {
        let Some(layout) = self
            .gfx
            .fonts
            .layout(text, style, w, align == Align::Center)
        else {
            return 0.0;
        };
        if color.a > 0.0 {
            // SAFETY: inside BeginDraw.
            unsafe {
                self.gfx.dc.DrawTextLayout(
                    Vector2 { X: x, Y: y },
                    &layout,
                    self.brush(color),
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                )
            }
        }
        metrics(&layout).height
    }

    /// Draw `text` centred in the column `r.w` wide, wrapping, but never
    /// below `r`'s bottom: lines that don't fit are cut and the last one
    /// that does ends in an ellipsis.
    pub fn text_clipped(&self, text: &str, style: Style, r: Rect, color: D2D1_COLOR_F) {
        if color.a <= 0.0 {
            return;
        }
        let Some(layout) = self.gfx.fonts.layout(text, style, r.w, true) else {
            return;
        };
        // SAFETY: our own layout and factory; inside BeginDraw.
        unsafe {
            if layout.SetMaxHeight(r.h.max(1.0)).is_err() {
                return;
            }
            let trimming = DWRITE_TRIMMING {
                granularity: DWRITE_TRIMMING_GRANULARITY_WORD,
                delimiter: 0,
                delimiterCount: 0,
            };
            let sign = self
                .gfx
                .fonts
                .factory
                .CreateEllipsisTrimmingSign(&layout)
                .ok();
            let _ = layout.SetTrimming(&trimming, sign.as_ref());
            self.gfx.dc.DrawTextLayout(
                Vector2 { X: r.x, Y: r.y },
                &layout,
                self.brush(color),
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
            )
        }
    }

    /// Draw one line of `text` centred vertically in `r` (buttons, labels).
    pub fn label(&self, text: &str, style: Style, r: Rect, align: Align, color: D2D1_COLOR_F) {
        let Some(layout) = self
            .gfx
            .fonts
            .layout(text, style, r.w, align == Align::Center)
        else {
            return;
        };
        if color.a <= 0.0 {
            return;
        }
        let h = metrics(&layout).height;
        // SAFETY: inside BeginDraw.
        unsafe {
            self.gfx.dc.DrawTextLayout(
                Vector2 {
                    X: r.x,
                    Y: r.y + (r.h - h) / 2.0,
                },
                &layout,
                self.brush(color),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            )
        }
    }

    /// The app icon in `r`; nothing if the exe has none.
    pub fn icon(&self, r: Rect, opacity: f32) {
        let Some(icon) = &self.gfx.icon else { return };
        if opacity <= 0.0 {
            return;
        }
        // SAFETY: inside BeginDraw.
        unsafe {
            self.gfx.dc.DrawBitmap(
                icon,
                Some(&r.d2d()),
                opacity.min(1.0),
                D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
                None,
                None,
            )
        }
    }
}

impl Gfx {
    /// Height of `text` wrapped to `w` DIPs.
    pub fn height(&self, text: &str, style: Style, w: f32) -> f32 {
        self.fonts
            .layout(text, style, w, true)
            .map_or(0.0, |l| metrics(&l).height)
    }

    /// Width of `text` on one line, in DIPs (for laying out links).
    pub fn measure(&self, text: &str, style: Style) -> f32 {
        self.fonts
            .layout(text, style, 10_000.0, false)
            .map_or(0.0, |l| metrics(&l).widthIncludingTrailingWhitespace)
    }
}
