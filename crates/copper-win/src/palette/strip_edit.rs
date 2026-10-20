//! The address in the live tab, tabs on top (`Browser.beginTabEdit`,
//! `commitTabEdit`, `cancelTabEdit`; `TabAddressField`).
//!
//! A click on the live, titled tab turns it into the address, short
//! (`Address.pretty`) and selected. Return goes there in that tab (an
//! address that cannot be turned into one keeps the field up, as the Mac's
//! refusal does); Escape, or the keyboard going elsewhere, puts the tab
//! back. A blank tab, or a pinned one, has no address to edit and gets the
//! plain field instead. `Ctrl+L` is always the plain field.

use crate::platform::Shell;
use copper_win_ui::{Event, NodeId};
use std::time::Duration;

/// How often the edit looks whether the keyboard is still in its field
/// (the Mac ends it from `controlTextDidEndEditing`; the toolkit has no
/// such word for a field the app does not own).
const LOOK: Duration = Duration::from_millis(150);

/// Begin editing the live tab in place; `false` when it has no address
/// to edit (the caller shows the plain field).
pub fn begin(shell: &Shell) -> bool {
    let Some(id) = shell.active_id() else {
        return false;
    };
    let Some((url, pinned)) = shell
        .browser()
        .tab(&id)
        .map(|t| (t.url.clone(), t.is_pinned()))
    else {
        return false;
    };
    if pinned || url.is_empty() || url.starts_with("about:blank") {
        return false;
    }
    let text = copper_core::address::pretty_str(&url);
    let chrome = shell.view().chrome().clone();
    chrome.edit(move |_, st| st.strip.begin_edit(id, text));
    shell.view().sync(shell);
    watch(shell);
    true
}

/// A field event from the strip's field (`false`: not its field).
pub fn strip_field(shell: &Shell, id: NodeId, e: &Event) -> bool {
    let chrome = shell.view().chrome().clone();
    let Some(text) = chrome
        .read(|ui, st| {
            (st.strip.field() == Some(id)).then(|| ui.field_text(id).unwrap_or("").to_string())
        })
        .flatten()
    else {
        return false;
    };
    match e {
        Event::FieldSubmit(_) => {
            // Stay put on what cannot be gone to, rather than throw the
            // edit away.
            if shell.destination(&text).is_some() {
                end(shell, false);
                shell.go(&text, false, false);
                shell.view().focus_page();
            }
        }
        Event::FieldCancel(_) => end(shell, true),
        _ => {}
    }
    true
}

/// Put the tab back; `focus`: give the keyboard back to the page.
fn end(shell: &Shell, focus: bool) {
    let chrome = shell.view().chrome().clone();
    chrome.edit(|_, st| {
        st.strip.end_edit();
    });
    shell.view().sync(shell);
    if focus {
        shell.view().focus_page();
    }
}

/// While the field shows, end the edit once the keyboard has gone
/// elsewhere in this window (a click on the page, another tab, a door).
fn watch(shell: &Shell) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, GetFocus};
    let weak = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        // The field is drawn on the window's next turn.
        let mut seen = false;
        for turn in 0.. {
            copper_win_rt::sleep(LOOK).await;
            let Some(shell) = weak.upgrade() else { break };
            let chrome = shell.view().chrome().clone();
            let hwnd = chrome.window().hwnd();
            let Some(state) = chrome.read(|ui, st| {
                st.strip.field().map(|f| {
                    // SAFETY: reads the calling thread's focus and
                    // active window.
                    let (active, focus) = unsafe { (GetActiveWindow(), GetFocus()) };
                    // Away from the app altogether is not looking away.
                    active != hwnd || (focus == hwnd && ui.tree.focused() == Some(f))
                })
            }) else {
                continue;
            };
            match state {
                None if !seen && turn < 10 => {}
                // Gone already (Return, Escape, another tab).
                None => break,
                Some(true) => seen = true,
                Some(false) => {
                    end(&shell, false);
                    break;
                }
            }
        }
    });
}

/// A press anywhere in the window but the field ends the edit, the way the
/// Mac's field gives up the keyboard to the page or another control (the
/// look above catches the keyboard moving on its own).
pub fn pointer(shell: &Shell) {
    const WM_NCLBUTTONDOWN: u32 = 0x00A1;
    const WM_NCRBUTTONDOWN: u32 = 0x00A4;
    const WM_LBUTTONDOWN: u32 = 0x0201;
    const WM_RBUTTONDOWN: u32 = 0x0204;
    const WM_MBUTTONDOWN: u32 = 0x0207;
    let weak = shell.downgrade();
    let hwnd = shell.view().chrome().window().hwnd();
    copper_win_rt::add_message_hook(move |m| {
        let client = matches!(m.message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN);
        if m.hwnd != hwnd.0 as isize
            || !(client || matches!(m.message, WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN))
        {
            return false;
        }
        let Some(shell) = weak.upgrade() else {
            return false;
        };
        let words = m.lparam as usize;
        let (x, y) = (
            f32::from((words & 0xFFFF) as u16 as i16),
            f32::from(((words >> 16) & 0xFFFF) as u16 as i16),
        );
        let away = shell
            .view()
            .chrome()
            .read(|ui, st| {
                st.strip.field().map(|f| {
                    let k = ui.scale().max(0.5);
                    !client
                        || !ui
                            .tree
                            .window_rect(f)
                            .contains(copper_win_ui::Point::new(x / k, y / k))
                })
            })
            .flatten()
            .unwrap_or(false);
        if away {
            let weak = shell.downgrade();
            copper_win_rt::spawn_local(async move {
                if let Some(shell) = weak.upgrade() {
                    end(&shell, false);
                }
            });
        }
        false
    });
}
