//! PNG → raw RGBA (`PageView::snapshot_bitmap`), with WIC on a worker
//! thread: WebView2 only hands out encoded frames, and decoding a whole
//! window must not stall the UI thread.

use copper_engine::{Bitmap, EngineError, Result};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Shell::SHCreateMemStream;

use super::worker;

/// Decode `png` into straight-alpha RGBA, off the UI thread.
pub(crate) async fn decode_png(png: Vec<u8>) -> Result<Bitmap> {
    worker::run("copper-bitmap", move || decode_now(&png))
        .await
        .unwrap_or_else(|| Err("the decoder thread failed".into()))
        .map_err(EngineError::Snapshot)
}

fn decode_now(png: &[u8]) -> std::result::Result<Bitmap, String> {
    // A thread of its own: COM is this thread's to set up and tear down.
    let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let r = decode_with_com(png).map_err(|e| format!("decoding the frame: {e}"));
    if init.is_ok() {
        unsafe { CoUninitialize() };
    }
    r
}

fn decode_with_com(png: &[u8]) -> windows::core::Result<Bitmap> {
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = SHCreateMemStream(Some(png))
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY))?;
        let decoder = factory.CreateDecoderFromStream(
            &stream,
            std::ptr::null(),
            WICDecodeMetadataCacheOnDemand,
        )?;
        let frame = decoder.GetFrame(0)?;
        // 32bppRGBA is WIC's straight (not premultiplied) RGBA.
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppRGBA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let (mut width, mut height) = (0u32, 0u32);
        converter.GetSize(&mut width, &mut height)?;
        let stride = width as usize * 4;
        let mut rgba = vec![0u8; stride * height as usize];
        converter.CopyPixels(std::ptr::null(), stride as u32, &mut rgba)?;
        Ok(Bitmap {
            width,
            height,
            stride,
            rgba,
        })
    }
}
