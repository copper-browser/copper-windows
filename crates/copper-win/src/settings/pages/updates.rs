//! Updates: which Copper this is, and the next one.

use crate::chrome::glyph;
use crate::platform::{Shell, Win};
use crate::settings::Page;
use crate::settings::form::Form;

/// Updates: which Copper this is, and the next one.
pub(super) struct Updates;

impl Page for Updates {
    fn id(&self) -> &'static str {
        "updates"
    }
    fn title(&self) -> &'static str {
        "Updates"
    }
    fn blurb(&self) -> &'static str {
        "Which Copper you have, and getting the next one."
    }
    fn glyph(&self) -> char {
        glyph::UPDATE
    }

    fn draw(&self, _shell: &Shell) -> Form {
        use copper_shell_core::updates;
        let st = updates::status::<Win>();
        let newer = updates::available::<Win>();
        let mut f = Form::default();
        let sec = f.section("Copper for Windows");
        sec.value("This Copper", None, env!("CARGO_PKG_VERSION"));
        let line = if st.checking {
            "Looking…".to_string()
        } else if let Some(e) = &st.error {
            format!("The last look failed: {e}")
        } else if let Some(v) = &newer {
            format!("Copper {v} is out")
        } else if let Some(when) = st.checked {
            format!(
                "Up to date — looked {}",
                copper_shell_core::format::ago::<crate::sys::calendar::WinCalendar>(
                    when,
                    copper_core::date::now_unix()
                )
            )
        } else {
            "Not looked yet".to_string()
        };
        sec.button("The newest", Some(&line), "check", "Check now", false);
        if newer.is_some() {
            sec.button(
                "Get it",
                Some("From the releases page"),
                "download",
                "Download",
                true,
            );
        }
        f
    }

    fn act(&self, shell: &Shell, action: &str, _value: &str) {
        use copper_shell_core::updates;
        match action {
            "check" => updates::check::<Win>(shell, true),
            "download" => {
                if let Err(e) = crate::sys::shell::open(&updates::release_page::<Win>()) {
                    tracing::warn!("release page: {e}");
                }
            }
            _ => {}
        }
    }
}
