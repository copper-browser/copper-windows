//! COM plumbing shared by the Windows modules: strings, out-parameters,
//! streams, event-token bookkeeping and futures over completion handlers.

use std::time::Duration;

use copper_engine::EngineError;
use copper_win_rt::OneshotRx;
use webview2_com::take_pwstr;
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{IStream, STATFLAG_NONAME, STREAM_SEEK_SET};
use windows::core::{BOOL, HSTRING, PWSTR};

use crate::util::{Either, race};

/// A string as WebView2 takes it.
pub(crate) fn hs(s: &str) -> HSTRING {
    HSTRING::from(s)
}

/// Read a `PWSTR` out-parameter (CoTaskMem-allocated) into a `String`.
pub(crate) fn get_string(
    f: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>,
) -> windows::core::Result<String> {
    let mut p = PWSTR::null();
    f(&mut p)?;
    Ok(take_pwstr(p))
}

/// Read a `BOOL` out-parameter.
pub(crate) fn get_bool(
    f: impl FnOnce(*mut BOOL) -> windows::core::Result<()>,
) -> windows::core::Result<bool> {
    let mut b = BOOL(0);
    f(&mut b)?;
    Ok(b.as_bool())
}

/// Read any plain out-parameter.
pub(crate) fn get<T: Default>(
    f: impl FnOnce(*mut T) -> windows::core::Result<()>,
) -> windows::core::Result<T> {
    let mut v = T::default();
    f(&mut v)?;
    Ok(v)
}

/// A WebView2 failure as an engine error, with what was being done.
pub(crate) fn other(what: &str, e: windows::core::Error) -> EngineError {
    EngineError::Other(format!("{what}: {e}"))
}

/// An empty in-memory stream (for `CapturePreview`).
pub(crate) fn memory_stream() -> windows::core::Result<IStream> {
    unsafe { CreateStreamOnHGlobal(Default::default(), true) }
}

/// Everything in `stream`, from the start.
pub(crate) fn read_stream(stream: &IStream) -> windows::core::Result<Vec<u8>> {
    unsafe {
        let mut stat = Default::default();
        stream.Stat(&mut stat, STATFLAG_NONAME)?;
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let len = usize::try_from(stat.cbSize).unwrap_or(usize::MAX);
        let mut buf = vec![0u8; len];
        let mut done = 0usize;
        while done < len {
            let mut read = 0u32;
            let want = u32::try_from(len - done).unwrap_or(u32::MAX);
            stream
                .Read(buf[done..].as_mut_ptr().cast(), want, Some(&mut read))
                .ok()?;
            if read == 0 {
                break;
            }
            done += read as usize;
        }
        buf.truncate(done);
        Ok(buf)
    }
}

/// Removers for registered event handlers; run (unregistering every
/// handler) on [`Tokens::clear`] or drop.
#[derive(Default)]
pub(crate) struct Tokens(Vec<Box<dyn FnOnce()>>);

impl Tokens {
    pub fn push(&mut self, remove: impl FnOnce() + 'static) {
        self.0.push(Box::new(remove));
    }

    pub fn clear(&mut self) {
        for remove in self.0.drain(..).rev() {
            remove();
        }
    }

    /// Let go of the removers without running them: the object they would
    /// call is gone and must not be called again (see
    /// `page::events::FrameHandlers`). Its handlers go when the object is
    /// released.
    pub fn forget(&mut self) {
        self.0.clear();
    }
}

impl Drop for Tokens {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Register an event handler on `$obj` and record how to remove it:
/// `on!(tokens, obj, NavigationStarting, NavigationStartingEventHandler, closure)`.
macro_rules! on {
    ($tokens:expr, $obj:expr, $event:ident, $handler:ident, $closure:expr) => {{
        let obj = $obj.clone();
        let mut token = 0i64;
        let handler = webview2_com::$handler::create(Box::new($closure));
        paste_add_remove!(obj, $event, handler, token, $tokens)
    }};
}

/// `add_<Event>` / `remove_<Event>` pair for [`on!`].
macro_rules! paste_add_remove {
    ($obj:ident, $event:ident, $handler:ident, $token:ident, $tokens:expr) => {{
        let r = unsafe { $crate::win::com::add_remove::$event::add(&$obj, &$handler, &mut $token) };
        match r {
            Ok(()) => {
                let o = $obj.clone();
                $tokens.push(move || unsafe {
                    let _ = $crate::win::com::add_remove::$event::remove(&o, $token);
                });
                Ok(())
            }
            Err(e) => Err(e),
        }
    }};
}

/// Wait for `rx`, at most `limit`; `None` on timeout or a dropped sender.
pub(crate) async fn within<T>(rx: OneshotRx<T>, limit: Duration) -> Option<T> {
    match race(rx, copper_win_rt::sleep(limit)).await {
        Either::Left(v) => v,
        Either::Right(()) => None,
    }
}

/// An already-finished `LocalFuture`.
pub(crate) fn ready<T: 'static>(v: T) -> copper_engine::LocalFuture<T> {
    Box::pin(std::future::ready(v))
}

/// `add_*`/`remove_*` adapters so [`on!`] can name an event once.
pub(crate) mod add_remove {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;

    macro_rules! event {
        ($event:ident, $iface:ty, $handler:ty, $add:ident, $remove:ident) => {
            #[allow(non_snake_case)]
            pub(crate) mod $event {
                use super::*;
                pub(crate) unsafe fn add(
                    obj: &$iface,
                    handler: &$handler,
                    token: &mut i64,
                ) -> windows::core::Result<()> {
                    unsafe { obj.$add(handler, token) }
                }
                pub(crate) unsafe fn remove(obj: &$iface, token: i64) -> windows::core::Result<()> {
                    unsafe { obj.$remove(token) }
                }
            }
        };
    }

    event!(
        NavigationStarting,
        ICoreWebView2,
        ICoreWebView2NavigationStartingEventHandler,
        add_NavigationStarting,
        remove_NavigationStarting
    );
    event!(
        ContentLoading,
        ICoreWebView2,
        ICoreWebView2ContentLoadingEventHandler,
        add_ContentLoading,
        remove_ContentLoading
    );
    event!(
        SourceChanged,
        ICoreWebView2,
        ICoreWebView2SourceChangedEventHandler,
        add_SourceChanged,
        remove_SourceChanged
    );
    event!(
        HistoryChanged,
        ICoreWebView2,
        ICoreWebView2HistoryChangedEventHandler,
        add_HistoryChanged,
        remove_HistoryChanged
    );
    event!(
        NavigationCompleted,
        ICoreWebView2,
        ICoreWebView2NavigationCompletedEventHandler,
        add_NavigationCompleted,
        remove_NavigationCompleted
    );
    event!(
        PermissionRequested,
        ICoreWebView2,
        ICoreWebView2PermissionRequestedEventHandler,
        add_PermissionRequested,
        remove_PermissionRequested
    );
    event!(
        ProcessFailed,
        ICoreWebView2,
        ICoreWebView2ProcessFailedEventHandler,
        add_ProcessFailed,
        remove_ProcessFailed
    );
    event!(
        WebMessageReceived,
        ICoreWebView2,
        ICoreWebView2WebMessageReceivedEventHandler,
        add_WebMessageReceived,
        remove_WebMessageReceived
    );
    event!(
        NewWindowRequested,
        ICoreWebView2,
        ICoreWebView2NewWindowRequestedEventHandler,
        add_NewWindowRequested,
        remove_NewWindowRequested
    );
    event!(
        DocumentTitleChanged,
        ICoreWebView2,
        ICoreWebView2DocumentTitleChangedEventHandler,
        add_DocumentTitleChanged,
        remove_DocumentTitleChanged
    );
    event!(
        ContainsFullScreenElementChanged,
        ICoreWebView2,
        ICoreWebView2ContainsFullScreenElementChangedEventHandler,
        add_ContainsFullScreenElementChanged,
        remove_ContainsFullScreenElementChanged
    );
    event!(
        WindowCloseRequested,
        ICoreWebView2,
        ICoreWebView2WindowCloseRequestedEventHandler,
        add_WindowCloseRequested,
        remove_WindowCloseRequested
    );
    event!(
        ScriptDialogOpening,
        ICoreWebView2,
        ICoreWebView2ScriptDialogOpeningEventHandler,
        add_ScriptDialogOpening,
        remove_ScriptDialogOpening
    );
    event!(
        WebResourceRequested,
        ICoreWebView2,
        ICoreWebView2WebResourceRequestedEventHandler,
        add_WebResourceRequested,
        remove_WebResourceRequested
    );
    event!(
        DOMContentLoaded,
        ICoreWebView2_2,
        ICoreWebView2DOMContentLoadedEventHandler,
        add_DOMContentLoaded,
        remove_DOMContentLoaded
    );
    event!(
        FrameCreated,
        ICoreWebView2_4,
        ICoreWebView2FrameCreatedEventHandler,
        add_FrameCreated,
        remove_FrameCreated
    );
    event!(
        DownloadStarting,
        ICoreWebView2_4,
        ICoreWebView2DownloadStartingEventHandler,
        add_DownloadStarting,
        remove_DownloadStarting
    );
    event!(
        IsMutedChanged,
        ICoreWebView2_8,
        ICoreWebView2IsMutedChangedEventHandler,
        add_IsMutedChanged,
        remove_IsMutedChanged
    );
    event!(
        IsDocumentPlayingAudioChanged,
        ICoreWebView2_8,
        ICoreWebView2IsDocumentPlayingAudioChangedEventHandler,
        add_IsDocumentPlayingAudioChanged,
        remove_IsDocumentPlayingAudioChanged
    );
    event!(
        ContextMenuRequested,
        ICoreWebView2_11,
        ICoreWebView2ContextMenuRequestedEventHandler,
        add_ContextMenuRequested,
        remove_ContextMenuRequested
    );
    event!(
        StatusBarTextChanged,
        ICoreWebView2_12,
        ICoreWebView2StatusBarTextChangedEventHandler,
        add_StatusBarTextChanged,
        remove_StatusBarTextChanged
    );
    event!(
        FaviconChanged,
        ICoreWebView2_15,
        ICoreWebView2FaviconChangedEventHandler,
        add_FaviconChanged,
        remove_FaviconChanged
    );
    event!(
        ZoomFactorChanged,
        ICoreWebView2Controller,
        ICoreWebView2ZoomFactorChangedEventHandler,
        add_ZoomFactorChanged,
        remove_ZoomFactorChanged
    );
    event!(
        MoveFocusRequested,
        ICoreWebView2Controller,
        ICoreWebView2MoveFocusRequestedEventHandler,
        add_MoveFocusRequested,
        remove_MoveFocusRequested
    );
    event!(
        GotFocus,
        ICoreWebView2Controller,
        ICoreWebView2FocusChangedEventHandler,
        add_GotFocus,
        remove_GotFocus
    );
    event!(
        LostFocus,
        ICoreWebView2Controller,
        ICoreWebView2FocusChangedEventHandler,
        add_LostFocus,
        remove_LostFocus
    );
    event!(
        AcceleratorKeyPressed,
        ICoreWebView2Controller,
        ICoreWebView2AcceleratorKeyPressedEventHandler,
        add_AcceleratorKeyPressed,
        remove_AcceleratorKeyPressed
    );
    event!(
        CursorChanged,
        ICoreWebView2CompositionController,
        ICoreWebView2CursorChangedEventHandler,
        add_CursorChanged,
        remove_CursorChanged
    );
    event!(
        FrameWebMessageReceived,
        ICoreWebView2Frame2,
        ICoreWebView2FrameWebMessageReceivedEventHandler,
        add_WebMessageReceived,
        remove_WebMessageReceived
    );
    event!(
        FrameDestroyed,
        ICoreWebView2Frame,
        ICoreWebView2FrameDestroyedEventHandler,
        add_Destroyed,
        remove_Destroyed
    );
    event!(
        FrameChildFrameCreated,
        ICoreWebView2Frame7,
        ICoreWebView2FrameChildFrameCreatedEventHandler,
        add_FrameCreated,
        remove_FrameCreated
    );
    event!(
        DevToolsProtocolEventReceived,
        ICoreWebView2DevToolsProtocolEventReceiver,
        ICoreWebView2DevToolsProtocolEventReceivedEventHandler,
        add_DevToolsProtocolEventReceived,
        remove_DevToolsProtocolEventReceived
    );
    event!(
        BytesReceivedChanged,
        ICoreWebView2DownloadOperation,
        ICoreWebView2BytesReceivedChangedEventHandler,
        add_BytesReceivedChanged,
        remove_BytesReceivedChanged
    );
    event!(
        StateChanged,
        ICoreWebView2DownloadOperation,
        ICoreWebView2StateChangedEventHandler,
        add_StateChanged,
        remove_StateChanged
    );
    event!(
        FindMatchCountChanged,
        ICoreWebView2Find,
        ICoreWebView2FindMatchCountChangedEventHandler,
        add_MatchCountChanged,
        remove_MatchCountChanged
    );
    event!(
        BrowserProcessExited,
        ICoreWebView2Environment5,
        ICoreWebView2BrowserProcessExitedEventHandler,
        add_BrowserProcessExited,
        remove_BrowserProcessExited
    );
}
