//! The save offer (`App.swift` `keepAsking`): a capsule rising from the
//! foot of the window, 30 above it in the bars' stack, with the question
//! in the Mac's words, the primary Save / Update (or Save to / Update in a
//! manager), Not now, and — for a password not kept before — Never here.
//! The password itself is never shown back.
//!
//! Offered once, answered once: Save keeps it in the store chosen (the
//! lane falls back to Credential Manager when a manager is locked or
//! read-only) and says how it went; Not now just goes; Never here adds the
//! site to `passwords.never` and says `Never for <host>`. The capsule
//! takes no keyboard focus from the page, and the lane takes it down when
//! the tab or the page changes.

use super::WinForms;
use crate::chrome::Chrome;
use crate::chrome::toast::{ask_pill, capsule};
use crate::platform::Shell;
use crate::settings::fill;
use copper_core::browser::TabId;
use copper_shell_core::forms::view::{OfferData, OfferView};
use copper_shell_core::forms::{self, Forms, rt};
use copper_win_ui::{Insets, Node, NodeId, Role, Token, Trim};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// The question's widest, before it is cut short (the Mac's one line).
const QUESTION_MAX: f32 = 520.0;

/// A save offer on show.
pub struct Offer {
    ticket: u64,
}

/// One capsule's nodes and what its buttons answer.
struct Shown {
    ticket: u64,
    node: NodeId,
    save: NodeId,
    later: NodeId,
    never: Option<NodeId>,
    forms: Weak<Forms<WinForms>>,
    chrome: Weak<Chrome>,
    tab: TabId,
    data: OfferData,
}

thread_local! {
    static SHOWN: RefCell<Vec<Shown>> = const { RefCell::new(Vec::new()) };
    static TICKETS: Cell<u64> = const { Cell::new(0) };
    /// Offers taken back before the window was free to build them.
    static WITHDRAWN: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

impl OfferView for Offer {
    fn remove(&self) {
        match take(|s| s.ticket == self.ticket) {
            Some(s) => lower(&s),
            None => WITHDRAWN.with(|w| w.borrow_mut().push(self.ticket)),
        }
    }
}

/// The first capsule `pick` chooses, out of the list.
fn take(pick: impl Fn(&Shown) -> bool) -> Option<Shown> {
    SHOWN.with(|v| {
        let mut v = v.borrow_mut();
        let i = v.iter().position(pick)?;
        Some(v.remove(i))
    })
}

/// Take a capsule's nodes away.
fn lower(s: &Shown) {
    let node = s.node;
    if let Some(c) = s.chrome.upgrade() {
        c.edit(move |ui, _| ui.tree.remove(node));
    }
}

/// Put an offer up for `data` on `tab`.
pub fn show(shell: &Shell, forms: &Rc<Forms<WinForms>>, tab: &TabId, data: OfferData) -> Offer {
    let ticket = TICKETS.with(|t| {
        t.set(t.get() + 1);
        t.get()
    });
    let (question, primary) =
        fill::question(&data.sent.user, &data.sent.host, data.target, data.changed);
    let new = !data.changed;
    let chrome = shell.view().chrome().clone();
    let weak_chrome = Rc::downgrade(&chrome);
    let forms = Rc::downgrade(forms);
    let tab = tab.clone();
    // Built when the window is free; the ticket finds it either way.
    chrome.edit(move |ui, st| {
        let gone = WITHDRAWN.with(|w| {
            let mut w = w.borrow_mut();
            let had = w.contains(&ticket);
            w.retain(|t| *t != ticket);
            had
        });
        if gone {
            return;
        }
        let node = ui.tree.insert(
            st.toast.bars(),
            0,
            ask_pill()
                .padding(Insets::new(16.0, 9.0, 12.0, 9.0))
                .hit()
                .role(Role::Group)
                .name(&question),
        );
        ui.tree.add(
            node,
            Node::text(&question, 12.5)
                .ink(Token::Ink)
                .trim(Trim::End)
                .max_width(QUESTION_MAX)
                .shrink(1.0)
                .role(Role::None),
        );
        let mut button = |label: &str, primary: bool| {
            let (b, t) = capsule(label, primary);
            let b = ui.tree.add(node, b.shrink(0.0));
            ui.tree.add(b, t);
            b
        };
        let save = button(&primary, true);
        let later = button("Not now", false);
        let never = new.then(|| button("Never here", false));
        SHOWN.with(|v| {
            v.borrow_mut().push(Shown {
                ticket,
                node,
                save,
                later,
                never,
                forms,
                chrome: weak_chrome,
                tab,
                data,
            })
        });
    });
    Offer { ticket }
}

/// A click on one of the offers' buttons: answer it.
pub fn click(shell: &Shell, id: NodeId) -> bool {
    let Some(s) = take(|s| s.save == id || s.later == id || s.never == Some(id)) else {
        return false;
    };
    lower(&s);
    // The caret goes back to the page the offer was over.
    shell.view().focus_page();
    let Some(f) = s.forms.upgrade() else {
        return true;
    };
    // The lane forgets the offer (its `remove` finds nothing left to do).
    f.drop_offer(&s.tab);
    if id == s.save {
        save(&f, shell, s.data);
    } else if Some(id) == s.never {
        let host = s.data.sent.host.trim().to_lowercase();
        forms::set_never(shell, &host, true);
        crate::settings::changed(shell, "passwords");
        shell.toast(&format!("Never for {host}"));
    }
    true
}

/// Keep the password the page sent, and say how it went — never the
/// password itself, in words or in the log.
fn save(f: &Rc<Forms<WinForms>>, shell: &Shell, data: OfferData) {
    let v = f.vault.clone();
    let shell = shell.downgrade();
    copper_win_rt::spawn_local(async move {
        let OfferData {
            sent,
            target,
            changed,
        } = data;
        let host = sent.host.clone();
        let r =
            rt::run(async move { v.save(target, &sent.host, &sent.user, &sent.password).await })
                .await
                .unwrap_or_else(|| Err(String::new()));
        let said = match r {
            Ok(()) => fill::saved(&host, target, changed),
            Err(e) => {
                tracing::warn!("saving a password for {host} was refused");
                fill::refused(target, &e)
            }
        };
        if let Some(s) = shell.upgrade() {
            s.toast(&said);
            crate::settings::changed(&s, "passwords");
        }
    });
}
