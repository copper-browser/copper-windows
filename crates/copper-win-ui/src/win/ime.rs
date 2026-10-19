//! IMM32: composition strings in, candidate window placement out.
//!
//! The field draws the composition itself, at the caret, with the IME's
//! clauses underlined (thick under the one being converted); the IME's own
//! composition window is switched off in `WM_IME_SETCONTEXT`. The candidate
//! list is placed just under the caret and told to keep clear of it
//! (`CFS_EXCLUDE`), so it never covers what is being typed.
//!
//! When no field has focus the window has no input context at all, so an
//! IME never starts composing into the void.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::UI::Input::Ime::{
    ATTR_TARGET_CONVERTED, ATTR_TARGET_NOTCONVERTED, CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT,
    COMPOSITIONFORM, CPS_CANCEL, CPS_COMPLETE, GCS_COMPATTR, GCS_COMPCLAUSE, GCS_COMPSTR,
    GCS_CURSORPOS, GCS_RESULTSTR, HIMC, IACE_DEFAULT, IME_COMPOSITION_STRING,
    ISC_SHOWUICOMPOSITIONWINDOW, ImmAssociateContextEx, ImmGetCompositionStringW, ImmGetContext,
    ImmNotifyIME, ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionWindow,
    NI_COMPOSITIONSTR,
};

use crate::edit::{Clause, byte_of_utf16};

/// What one `WM_IME_COMPOSITION` carried.
#[derive(Debug, Default)]
pub struct Update {
    /// Final text to insert.
    pub result: Option<String>,
    /// The composition in progress: text, caret (bytes), clauses.
    pub composition: Option<(String, usize, Vec<Clause>)>,
}

/// Runs `f` with the window's input context, releasing it after.
fn with_context<R>(hwnd: HWND, f: impl FnOnce(HIMC) -> R) -> Option<R> {
    // SAFETY: the context is released on the same window it came from.
    unsafe {
        let himc = ImmGetContext(hwnd);
        if himc.is_invalid() {
            return None;
        }
        let r = f(himc);
        let _ = ImmReleaseContext(hwnd, himc);
        Some(r)
    }
}

fn read_bytes(himc: HIMC, kind: IME_COMPOSITION_STRING) -> Vec<u8> {
    // SAFETY: the first call sizes the buffer the second fills.
    unsafe {
        let n = ImmGetCompositionStringW(himc, kind, None, 0);
        if n <= 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; n as usize];
        let got = ImmGetCompositionStringW(himc, kind, Some(buf.as_mut_ptr().cast()), n as u32);
        buf.truncate(got.max(0) as usize);
        buf
    }
}

fn read_string(himc: HIMC, kind: IME_COMPOSITION_STRING) -> String {
    let b = read_bytes(himc, kind);
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Read what changed, per the `GCS_*` flags in `lparam`.
pub fn read(hwnd: HWND, lparam: LPARAM) -> Update {
    let flags = lparam.0 as u32;
    with_context(hwnd, |himc| {
        let mut u = Update::default();
        if flags & GCS_RESULTSTR.0 != 0 {
            u.result = Some(read_string(himc, GCS_RESULTSTR));
        }
        if flags & GCS_COMPSTR.0 != 0 {
            let text = read_string(himc, GCS_COMPSTR);
            // SAFETY: GCS_CURSORPOS returns the cursor position directly.
            let cursor16 = if flags & GCS_CURSORPOS.0 != 0 {
                unsafe { ImmGetCompositionStringW(himc, GCS_CURSORPOS, None, 0) }.max(0) as usize
            } else {
                text.encode_utf16().count()
            };
            let caret = byte_of_utf16(&text, cursor16);
            let attrs = if flags & GCS_COMPATTR.0 != 0 {
                read_bytes(himc, GCS_COMPATTR)
            } else {
                Vec::new()
            };
            let clause_bytes = if flags & GCS_COMPCLAUSE.0 != 0 {
                read_bytes(himc, GCS_COMPCLAUSE)
            } else {
                Vec::new()
            };
            let bounds: Vec<usize> = clause_bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c) as usize)
                .collect();
            let mut clauses = Vec::new();
            for w in bounds.windows(2) {
                let (a16, b16) = (w[0], w[1]);
                if b16 <= a16 {
                    continue;
                }
                let attr = attrs.get(a16).copied().unwrap_or(0) as u32;
                clauses.push(Clause {
                    range: byte_of_utf16(&text, a16)..byte_of_utf16(&text, b16),
                    target: attr == ATTR_TARGET_CONVERTED || attr == ATTR_TARGET_NOTCONVERTED,
                });
            }
            u.composition = Some((text, caret, clauses));
        }
        u
    })
    .unwrap_or_default()
}

/// Keep the IME's own composition window hidden; we draw the composition.
pub fn context_lparam(lparam: LPARAM) -> LPARAM {
    LPARAM(lparam.0 & !(ISC_SHOWUICOMPOSITIONWINDOW as isize))
}

/// Place the IME's windows by the caret: `caret` in client pixels.
pub fn place(hwnd: HWND, caret: RECT) {
    with_context(hwnd, |himc| {
        let at = POINT {
            x: caret.left,
            y: caret.bottom,
        };
        let comp = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT {
                x: caret.left,
                y: caret.top,
            },
            rcArea: RECT::default(),
        };
        let cand = CANDIDATEFORM {
            dwIndex: 0,
            dwStyle: CFS_EXCLUDE,
            ptCurrentPos: at,
            rcArea: caret,
        };
        // SAFETY: plain structs on a live context.
        unsafe {
            let _ = ImmSetCompositionWindow(himc, &comp);
            let _ = ImmSetCandidateWindow(himc, &cand);
        }
    });
}

/// Give the window an input context (a field has focus) or take it away.
pub fn enable(hwnd: HWND, on: bool) {
    // SAFETY: associating the default context, or none, with our window.
    unsafe {
        let _ = ImmAssociateContextEx(hwnd, HIMC::default(), if on { IACE_DEFAULT } else { 0 });
    }
}

/// Commit whatever is being composed (a click elsewhere in the field).
pub fn complete(hwnd: HWND) {
    with_context(hwnd, |himc| {
        // SAFETY: notification on a live context.
        unsafe {
            let _ = ImmNotifyIME(himc, NI_COMPOSITIONSTR, CPS_COMPLETE, 0);
        }
    });
}

/// Throw the composition away (focus left the field).
pub fn cancel(hwnd: HWND) {
    with_context(hwnd, |himc| {
        // SAFETY: notification on a live context.
        unsafe {
            let _ = ImmNotifyIME(himc, NI_COMPOSITIONSTR, CPS_CANCEL, 0);
        }
    });
}
