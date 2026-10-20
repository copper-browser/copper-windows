//! Cloud: connect to a Copper Cloud, sign in, choose what syncs — the page
//! itself is [`crate::cloud::page`]'s, which draws into Settings' list.

use crate::chrome::glyph;
use crate::platform::Shell;
use crate::settings::Page;
use crate::settings::form::Form;
use copper_win_ui::win::Ui;
use copper_win_ui::{Event, NodeId};

/// Cloud: where this PC stands with Copper Cloud.
pub(super) struct Cloud;

impl Page for Cloud {
    fn id(&self) -> &'static str {
        crate::cloud::page::ID
    }
    fn title(&self) -> &'static str {
        "Cloud"
    }
    fn blurb(&self) -> &'static str {
        "Connect, sign in, then choose what follows you between devices."
    }
    fn glyph(&self) -> char {
        glyph::CLOUD
    }

    fn draw(&self, _shell: &Shell) -> Form {
        Form::default()
    }

    fn act(&self, _shell: &Shell, _action: &str, _value: &str) {}

    fn body(&self, shell: &Shell) -> Option<Box<dyn FnOnce(&mut Ui, NodeId)>> {
        crate::cloud::page::body(shell)
    }

    fn click(&self, shell: &Shell, id: NodeId) -> bool {
        crate::cloud::page::click(shell, id)
    }

    fn field(&self, shell: &Shell, id: NodeId, e: &Event) -> bool {
        crate::cloud::page::field(shell, id, e)
    }

    fn left(&self) {
        crate::cloud::page::left();
    }
}
