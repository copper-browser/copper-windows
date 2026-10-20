//! What the Canvas card holds between frames (`CanvasUI.shared`): open or
//! not, its mode, what each of its nodes does, the text typed into its
//! fields, and the answers on their way.

use super::share;
use copper_cloud::canvas::Member;
use copper_win_ui::NodeId;
use std::cell::RefCell;
use std::collections::HashMap;

/// What the card shows (`CanvasUI.Mode`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    List,
    New,
    Rename(String),
    Invite(String),
    Share(String),
    Members(String),
    Delete(String),
}

impl Mode {
    /// The canvas the mode is about.
    pub fn canvas(&self) -> Option<&str> {
        match self {
            Mode::List | Mode::New => None,
            Mode::Rename(id)
            | Mode::Invite(id)
            | Mode::Share(id)
            | Mode::Members(id)
            | Mode::Delete(id) => Some(id),
        }
    }

    /// A form or a confirmation inside the card: Escape goes back to the
    /// list rather than closing it.
    pub fn is_form(&self) -> bool {
        !matches!(self, Mode::List)
    }
}

/// A field of the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// New / Rename: "Canvas name".
    Name,
    /// Invite: "name@example.com".
    Email,
    /// Share: "Search people or type an email".
    Query,
}

/// What a node of the card does when clicked (and, for focus, which one it
/// is across a redraw).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Act {
    Scrim,
    Back,
    Open(String),
    NewMode,
    Settings,
    Join(String),
    Decline(String),
    Submit,
    Cancel,
    Confirm,
    Field(FieldKind),
    // The Share card.
    CopyLink,
    CopyWeb,
    ResetLink,
    InviteTo(String),
    Resend(String),
    Withdraw(String),
    Remove(String),
    NewShared,
    Connect,
}

/// The card.
#[derive(Default)]
pub struct Card {
    pub open: bool,
    pub mode: Mode,
    /// The see-through layer under the card (a click on it closes).
    pub scrim: Option<NodeId>,
    pub card: Option<NodeId>,
    pub hits: HashMap<NodeId, Act>,
    /// Canvas rows, for their menu.
    pub rows: HashMap<NodeId, String>,
    pub fields: HashMap<NodeId, FieldKind>,
    pub texts: HashMap<FieldKind, String>,
    /// The form's action, enabled while its field has something to send.
    pub submit: Option<(NodeId, FieldKind)>,
    /// What just happened, said under the list (`ui.note`).
    pub note: Option<String>,
    pub working: bool,
    pub failure: Option<String>,
    /// Invitations being answered: id → accepting.
    pub answering: HashMap<String, bool>,
    pub answer_failed: HashMap<String, String>,
    /// The Members view's list, or why it isn't there.
    pub members: Option<Result<Vec<Member>, String>>,
    pub share: Option<share::Model>,
    /// What to focus once drawn.
    pub focus: Option<Act>,
    /// The first row: the keyboard starts there.
    pub first: Option<NodeId>,
}

thread_local! {
    pub static CARD: RefCell<Card> = RefCell::new(Card::default());
}

/// Run `f` with the card.
pub fn with<R>(f: impl FnOnce(&mut Card) -> R) -> R {
    CARD.with(|c| f(&mut c.borrow_mut()))
}

/// The card is up.
pub fn is_open() -> bool {
    CARD.with(|c| c.try_borrow().map(|c| c.open).unwrap_or(false))
}
