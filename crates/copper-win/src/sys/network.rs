//! Whether the machine is online: Windows' Network List Manager, the same
//! answer the taskbar's network icon gives.

use std::cell::RefCell;
use windows::Win32::Networking::NetworkListManager::{INetworkListManager, NetworkListManager};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

thread_local! {
    /// Made once on the UI thread (an STA: OLE is initialised there).
    static NLM: RefCell<Option<INetworkListManager>> = const { RefCell::new(None) };
}

/// `INetworkListManager::IsConnectedToInternet`. When Windows cannot say
/// (the service is off), Copper assumes it is online: a page's own error
/// says more than a guess would.
pub fn is_online() -> bool {
    NLM.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            // SAFETY: creating an in-box COM object on an initialised thread.
            *slot = unsafe { CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL) }.ok();
        }
        match slot.as_ref() {
            // SAFETY: a plain query on a live object.
            Some(nlm) => unsafe { nlm.IsConnectedToInternet() }.map_or(true, |b| b.as_bool()),
            None => true,
        }
    })
}
