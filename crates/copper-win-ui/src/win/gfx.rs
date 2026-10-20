//! The device chain: Direct3D 11 → DXGI → Direct2D 1.1 → DirectComposition.
//!
//! One [`Gfx`] serves the main window and its popups. Every device-bound
//! resource (surfaces, brushes, baked shadows) is tagged with the
//! [`Gfx::generation`] it was made under; when the GPU is lost (a driver
//! update, a TDR, a remote-desktop switch) the whole chain is rebuilt, the
//! generation moves on and everything is redrawn.
//!
//! The composition device is made without a rendering device: surfaces
//! come from a surface factory of the Direct2D device, so the rendering
//! device can change under a live visual tree ([`Gfx::rebind`]). A window
//! draws its first frame on WARP — a hardware device takes ~200 ms to make
//! (a discrete GPU's driver loading), WARP ~25 ms — and moves to the
//! hardware device as soon as it is ready, with no blank frame in between:
//! each surface is replaced by its redrawn copy in one commit.

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct2D::{
    D2D1_DEBUG_LEVEL_NONE, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_OPTIONS,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1CreateFactory, ID2D1DeviceContext, ID2D1Factory1,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL,
    D3D_FEATURE_LEVEL_9_1, D3D_FEATURE_LEVEL_9_2, D3D_FEATURE_LEVEL_9_3, D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionSurfaceFactory,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_DEVICE_HUNG, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
    DXGI_ERROR_DRIVER_INTERNAL_ERROR, IDXGIDevice,
};
use windows::core::{Error, HRESULT, Interface, Result};

/// `D2DERR_RECREATE_TARGET`: the render target must be rebuilt.
const D2DERR_RECREATE_TARGET: HRESULT = HRESULT(0x8899_000C_u32 as i32);
/// `DCOMPOSITION_ERROR_SURFACE_NOT_BEING_RENDERED` and friends say the
/// device is gone when they come back from a draw that used to work.
const DCOMPOSITION_ERROR_ACCESS_DENIED: HRESULT = HRESULT(0x8898_0802_u32 as i32);

/// A Direct3D device made off the UI thread (it is free-threaded), handed
/// over when the window needs it.
pub struct D3d(pub ID3D11Device);

// SAFETY: ID3D11Device is free-threaded unless created with
// D3D11_CREATE_DEVICE_SINGLETHREADED, which this module never passes.
unsafe impl Send for D3d {}

/// The devices.
pub struct Gfx {
    pub d3d: ID3D11Device,
    pub d2d_factory: ID2D1Factory1,
    pub dcomp: IDCompositionDesktopDevice,
    /// Makes the layers' surfaces on the current rendering device.
    pub surfaces: IDCompositionSurfaceFactory,
    /// A context of its own for baking shadows off the surfaces.
    pub offscreen: ID2D1DeviceContext,
    /// Bumped on every rebuild and on every [`Gfx::rebind`]: what was
    /// drawn under another generation is drawn again.
    pub generation: u32,
    /// The generation `dcomp` was made in (visuals and targets made under
    /// another are made again).
    pub comp_generation: u32,
}

const LEVELS: [D3D_FEATURE_LEVEL; 7] = [
    D3D_FEATURE_LEVEL_11_1,
    D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_10_0,
    D3D_FEATURE_LEVEL_9_3,
    D3D_FEATURE_LEVEL_9_2,
    D3D_FEATURE_LEVEL_9_1,
];

/// A WARP (software) device: quick to make, for the first frame while the
/// hardware device is still being made.
pub fn create_warp() -> Result<D3d> {
    d3d_with(D3D_DRIVER_TYPE_WARP).map(D3d)
}

fn d3d_with(driver: D3D_DRIVER_TYPE) -> Result<ID3D11Device> {
    let mut device = None;
    // SAFETY: plain device creation; out-pointer is a local Option.
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&LEVELS),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(|| Error::from_hresult(DXGI_ERROR_DEVICE_REMOVED))
}

/// A hardware device with BGRA support, or WARP when there is no GPU (a
/// basic-display VM, a remote session without one).
pub fn create_d3d() -> Result<D3d> {
    d3d_with(D3D_DRIVER_TYPE_HARDWARE)
        .inspect(|_| HARDWARE_MADE.store(true, std::sync::atomic::Ordering::Relaxed))
        .or_else(|_| {
            ON_WARP.store(true, std::sync::atomic::Ordering::Relaxed);
            d3d_with(D3D_DRIVER_TYPE_WARP)
        })
        .map(D3d)
}

/// Whether a hardware device was made in this process (its driver is
/// loaded: the next is quick to make).
pub static HARDWARE_MADE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a device was made on WARP (no hardware device to be had).
pub static ON_WARP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Gfx {
    /// Build the chain on top of `d3d` (or a fresh device).
    pub fn new(d3d: Option<D3d>, generation: u32) -> Result<Gfx> {
        let d3d = match d3d {
            Some(d) => d.0,
            None => create_d3d()?.0,
        };
        let options = D2D1_FACTORY_OPTIONS {
            debugLevel: D2D1_DEBUG_LEVEL_NONE,
        };
        // SAFETY: standard factory/device creation with valid arguments.
        unsafe {
            let d2d_factory: ID2D1Factory1 =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&options))?;
            let dcomp: IDCompositionDesktopDevice =
                DCompositionCreateDevice2(None::<&windows::core::IUnknown>)?;
            Self::on(d3d, d2d_factory, dcomp, generation, generation)
        }
    }

    /// The same composition device (visuals, targets, hosted pages all
    /// stay) on another rendering device: the layers' surfaces are made
    /// again on `d3d` and redrawn at the next frame.
    pub fn rebind(&self, d3d: D3d) -> Result<Gfx> {
        Self::on(
            d3d.0,
            self.d2d_factory.clone(),
            self.dcomp.clone(),
            self.generation.wrapping_add(1),
            self.comp_generation,
        )
    }

    fn on(
        d3d: ID3D11Device,
        d2d_factory: ID2D1Factory1,
        dcomp: IDCompositionDesktopDevice,
        generation: u32,
        comp_generation: u32,
    ) -> Result<Gfx> {
        let dxgi: IDXGIDevice = d3d.cast()?;
        // SAFETY: device creation with valid arguments.
        unsafe {
            let d2d = d2d_factory.CreateDevice(&dxgi)?;
            // Surfaces from a factory of the D2D device hand back D2D
            // contexts from BeginDraw.
            let surfaces = dcomp.CreateSurfaceFactory(&d2d)?;
            let offscreen = d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            Ok(Gfx {
                d3d,
                d2d_factory,
                dcomp,
                surfaces,
                offscreen,
                generation,
                comp_generation,
            })
        }
    }

    /// Whether the GPU went away under us.
    pub fn is_lost(&self) -> bool {
        // SAFETY: a status query on a live device.
        unsafe { self.d3d.GetDeviceRemovedReason().is_err() }
    }
}

/// Whether `e` means "rebuild the devices" rather than a bug.
pub fn is_device_loss(e: &Error) -> bool {
    let c = e.code();
    c == DXGI_ERROR_DEVICE_REMOVED
        || c == DXGI_ERROR_DEVICE_RESET
        || c == DXGI_ERROR_DEVICE_HUNG
        || c == DXGI_ERROR_DRIVER_INTERNAL_ERROR
        || c == D2DERR_RECREATE_TARGET
        || c == DCOMPOSITION_ERROR_ACCESS_DENIED
}
