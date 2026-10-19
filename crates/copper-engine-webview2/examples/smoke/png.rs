//! PNG-encoding a `snapshot_bitmap` on a worker thread (WIC), the way a
//! canvas frame poster does: the UI thread never encodes.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use copper_engine::Bitmap;
use windows::Win32::Foundation::GENERIC_WRITE;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA,
    IWICBitmapFrameEncode, IWICImagingFactory, WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::core::HSTRING;

/// What the worker did.
pub struct Encoded {
    pub bytes: u64,
    pub took: Duration,
    pub thread: String,
}

struct Slot<T> {
    value: Option<T>,
    waker: Option<Waker>,
}

/// Resolves with what a worker thread produced (`copper_win_rt`'s wakers
/// may be woken from any thread).
pub struct FromThread<T> {
    slot: Arc<Mutex<Slot<T>>>,
}

impl<T> Future for FromThread<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut s = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        match s.value.take() {
            Some(v) => Poll::Ready(v),
            None => {
                s.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Encode `bitmap` into `path` on a new thread.
pub fn encode(bitmap: Bitmap, path: PathBuf) -> FromThread<Result<Encoded, String>> {
    let slot = Arc::new(Mutex::new(Slot {
        value: None,
        waker: None,
    }));
    let worker = slot.clone();
    let spawned = std::thread::Builder::new()
        .name("smoke-png".into())
        .spawn(move || {
            let r = encode_now(&bitmap, &path);
            let waker = {
                let mut s = worker.lock().unwrap_or_else(|e| e.into_inner());
                s.value = Some(r);
                s.waker.take()
            };
            if let Some(w) = waker {
                w.wake();
            }
        });
    if let Err(e) = spawned {
        slot.lock().unwrap_or_else(|e| e.into_inner()).value = Some(Err(e.to_string()));
    }
    FromThread { slot }
}

fn encode_now(b: &Bitmap, path: &Path) -> Result<Encoded, String> {
    let t = Instant::now();
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    hr.ok().map_err(|e| format!("COM: {e}"))?;
    let r = write_png(b, path);
    unsafe { CoUninitialize() };
    r.map_err(|e| e.to_string())?;
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    Ok(Encoded {
        bytes,
        took: t.elapsed(),
        thread,
    })
}

fn write_png(b: &Bitmap, path: &Path) -> windows::core::Result<()> {
    // WIC's PNG encoder takes BGRA; the bitmap is straight RGBA.
    let mut bgra = Vec::with_capacity(b.width as usize * 4 * b.height as usize);
    for y in 0..b.height {
        if let Some(row) = b.row(y) {
            for px in row.as_chunks::<4>().0 {
                bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
    }
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromFilename(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame: Option<IWICBitmapFrameEncode> = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))?;
        frame.Initialize(None)?;
        frame.SetSize(b.width, b.height)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WritePixels(b.height, b.width * 4, &bgra)?;
        frame.Commit()?;
        encoder.Commit()?;
    }
    Ok(())
}
