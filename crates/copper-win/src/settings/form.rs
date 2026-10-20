//! What a Settings page says, as plain data: sections of lines, each with
//! one control. A page describes itself from what the shell knows
//! ([`super::Page::draw`]); the panel turns the description into nodes and
//! hands clicks back as the control's action ([`super::Page::act`]). Pages
//! never touch the window, so they cannot trip over its borrow, and adding
//! one is only words and actions.
//!
//! Everything past the original `section` / `note` / `value` / `switch` /
//! `choice` / `button` builders is additive: a line's search words and
//! anchor ([`Section::keywords`], [`Section::anchor`]), a line that can't
//! be used now ([`Section::disabled`]), red or green words
//! ([`Section::tone`]), a section's note and stacked cards
//! ([`Section::footnote`], [`Form::more`]), and the controls the Mac's
//! pages use beyond those (text and secret fields, several pills, a popup
//! choice, a checkmark). Builders are the only way pages make these, so a
//! page written against the first six still builds unchanged.

/// A page's description.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Form {
    pub sections: Vec<Section>,
}

/// A titled card of lines (`SettingsSection`).
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub lines: Vec<Line>,
    /// A sentence under the card: where something is kept, what it costs
    /// (`SettingsNote`).
    pub note: Option<String>,
    /// False: the lines sit on the page with no card around them (a row of
    /// pills, a lone sentence).
    pub carded: bool,
    /// Another card of the section above, 12 under it with no title of its
    /// own (`Password managers`' Bitwarden and 1Password cards).
    pub continues: bool,
    /// What search lands on for the whole card (`settingsAnchor(card:)`).
    pub anchor: Option<String>,
}

/// One thing to set or do (`Line`): words on the left, a control on the
/// right.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub title: String,
    pub detail: Option<String>,
    pub control: Control,
    /// What search scrolls to (`settingsAnchor`); `None`: the title.
    pub anchor: Option<String>,
    /// Other words search finds the line by.
    pub keywords: Vec<String>,
    /// Shown faded, its control out of reach (`.disabled`, 45%).
    pub disabled: bool,
    /// The detail's colour.
    pub tone: Tone,
    /// The detail in a fixed-pitch face (a path, an address, a command).
    pub mono: bool,
}

/// What the detail under a line's title says about its state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Plain,
    /// Something went wrong.
    Error,
    /// Something worked.
    Good,
}

/// One pill of several on a line ([`Control::Buttons`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Pill {
    pub action: String,
    pub label: String,
    /// The one thing the line is for: filled in ink.
    pub filled: bool,
    /// False: shown faded and not pressable (`Checking…`).
    pub enabled: bool,
}

/// A line's control. `action` is the page's own name for what it does.
#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    /// Words only.
    None,
    /// A short value, muted (a version, a port).
    Value(String),
    /// On or off.
    Switch { action: String, on: bool },
    /// One of a few, side by side (`Segmented`): `(value, label)`.
    Choice {
        action: String,
        options: Vec<(String, String)>,
        chosen: String,
    },
    /// A button (`Pill`); `filled` for the one thing the line is for.
    Button {
        action: String,
        label: String,
        filled: bool,
    },
    /// Several pills in a row, in order.
    Buttons(Vec<Pill>),
    /// One of many, behind a popup (`Picker(.menu)`): `(value, label)`.
    Pick {
        action: String,
        options: Vec<(String, String)>,
        chosen: String,
    },
    /// A text box. Each edit is `act(action, text)` with no redraw (the
    /// caret stays put); Enter is `act(submit, text)` and a redraw.
    Field {
        action: String,
        submit: String,
        text: String,
        placeholder: String,
        /// Shown as dots, never copied (keys, master passwords).
        secret: bool,
        /// Its width; `None`: what the line leaves.
        width: Option<f32>,
        mono: bool,
    },
    /// A checkmark: the thing is already so (Copper is the default browser).
    Check,
}

impl Form {
    /// Start a section titled `title`.
    pub fn section(&mut self, title: &str) -> &mut Section {
        self.sections.push(Section {
            title: title.to_string(),
            lines: Vec::new(),
            note: None,
            carded: true,
            continues: false,
            anchor: None,
        });
        self.sections.last_mut().expect("just pushed")
    }

    /// Another card under the section before, with no title of its own.
    pub fn more(&mut self) -> &mut Section {
        let s = self.section("");
        s.continues = true;
        s
    }
}

impl Section {
    fn push(&mut self, title: &str, detail: Option<&str>, control: Control) -> &mut Section {
        self.lines.push(Line {
            title: title.to_string(),
            detail: detail.map(str::to_string),
            control,
            anchor: None,
            keywords: Vec::new(),
            disabled: false,
            tone: Tone::Plain,
            mono: false,
        });
        self
    }

    fn last(&mut self) -> Option<&mut Line> {
        self.lines.last_mut()
    }

    /// A line with no control.
    pub fn note(&mut self, title: &str, detail: Option<&str>) -> &mut Section {
        self.push(title, detail, Control::None)
    }

    /// A line showing `value`.
    pub fn value(&mut self, title: &str, detail: Option<&str>, value: &str) -> &mut Section {
        self.push(title, detail, Control::Value(value.to_string()))
    }

    /// A switch.
    pub fn switch(
        &mut self,
        title: &str,
        detail: Option<&str>,
        action: &str,
        on: bool,
    ) -> &mut Section {
        self.push(
            title,
            detail,
            Control::Switch {
                action: action.to_string(),
                on,
            },
        )
    }

    /// A choice of `options` (`(value, label)`).
    pub fn choice(
        &mut self,
        title: &str,
        detail: Option<&str>,
        action: &str,
        options: &[(&str, &str)],
        chosen: &str,
    ) -> &mut Section {
        self.push(
            title,
            detail,
            Control::Choice {
                action: action.to_string(),
                options: pairs(options),
                chosen: chosen.to_string(),
            },
        )
    }

    /// A button.
    pub fn button(
        &mut self,
        title: &str,
        detail: Option<&str>,
        action: &str,
        label: &str,
        filled: bool,
    ) -> &mut Section {
        self.push(
            title,
            detail,
            Control::Button {
                action: action.to_string(),
                label: label.to_string(),
                filled,
            },
        )
    }

    /// Several pills: `(action, label, filled)`, all pressable.
    pub fn buttons(
        &mut self,
        title: &str,
        detail: Option<&str>,
        pills: &[(&str, &str, bool)],
    ) -> &mut Section {
        let pills = pills
            .iter()
            .map(|(action, label, filled)| Pill {
                action: action.to_string(),
                label: label.to_string(),
                filled: *filled,
                enabled: true,
            })
            .collect();
        self.push(title, detail, Control::Buttons(pills))
    }

    /// Several pills, each saying whether it can be pressed now.
    pub fn pills(&mut self, title: &str, detail: Option<&str>, pills: Vec<Pill>) -> &mut Section {
        self.push(title, detail, Control::Buttons(pills))
    }

    /// One of many `options` behind a popup.
    pub fn pick(
        &mut self,
        title: &str,
        detail: Option<&str>,
        action: &str,
        options: &[(&str, &str)],
        chosen: &str,
    ) -> &mut Section {
        self.push(
            title,
            detail,
            Control::Pick {
                action: action.to_string(),
                options: pairs(options),
                chosen: chosen.to_string(),
            },
        )
    }

    /// A text box holding `text`; `action` on each edit, `submit` on Enter.
    pub fn field(
        &mut self,
        title: &str,
        detail: Option<&str>,
        action: &str,
        text: &str,
        placeholder: &str,
    ) -> &mut Section {
        self.push(
            title,
            detail,
            Control::Field {
                action: action.to_string(),
                submit: action.to_string(),
                text: text.to_string(),
                placeholder: placeholder.to_string(),
                secret: false,
                width: Some(crate::settings::look::metric::FIELD_W),
                mono: false,
            },
        )
    }

    /// A text box that shows dots, never its text.
    pub fn secret(
        &mut self,
        title: &str,
        detail: Option<&str>,
        action: &str,
        text: &str,
        placeholder: &str,
    ) -> &mut Section {
        self.field(title, detail, action, text, placeholder);
        if let Some(Line {
            control: Control::Field { secret, .. },
            ..
        }) = self.last()
        {
            *secret = true;
        }
        self
    }

    /// A checkmark: already so.
    pub fn check(&mut self, title: &str, detail: Option<&str>) -> &mut Section {
        self.push(title, detail, Control::Check)
    }

    // — about the last line —

    /// What search scrolls to for the last line.
    pub fn anchor(&mut self, anchor: &str) -> &mut Section {
        if let Some(l) = self.last() {
            l.anchor = Some(anchor.to_string());
        }
        self
    }

    /// Other words search finds the last line by.
    pub fn keywords(&mut self, words: &[&str]) -> &mut Section {
        if let Some(l) = self.last() {
            l.keywords = words.iter().map(|w| w.to_string()).collect();
        }
        self
    }

    /// The last line can't be used now.
    pub fn disabled(&mut self, off: bool) -> &mut Section {
        if let Some(l) = self.last() {
            l.disabled = off;
        }
        self
    }

    /// The last line's detail says something went wrong, or worked.
    pub fn tone(&mut self, tone: Tone) -> &mut Section {
        if let Some(l) = self.last() {
            l.tone = tone;
        }
        self
    }

    /// The last line's detail (and field) in a fixed-pitch face.
    pub fn mono(&mut self) -> &mut Section {
        if let Some(l) = self.last() {
            l.mono = true;
            if let Control::Field { mono, .. } = &mut l.control {
                *mono = true;
            }
        }
        self
    }

    /// The last line's field: what Enter does (`submit`), and how wide it
    /// is (`None`: the room the line leaves).
    pub fn on_submit(&mut self, action: &str, width: Option<f32>) -> &mut Section {
        if let Some(Line {
            control: Control::Field {
                submit, width: w, ..
            },
            ..
        }) = self.last()
        {
            *submit = action.to_string();
            *w = width;
        }
        self
    }

    // — about the section —

    /// A sentence under the card.
    pub fn footnote(&mut self, text: &str) -> &mut Section {
        self.note = Some(text.to_string());
        self
    }

    /// No card around the lines.
    pub fn uncarded(&mut self) -> &mut Section {
        self.carded = false;
        self
    }

    /// What search lands on for the whole card.
    pub fn card_anchor(&mut self, anchor: &str) -> &mut Section {
        self.anchor = Some(anchor.to_string());
        self
    }
}

fn pairs(options: &[(&str, &str)]) -> Vec<(String, String)> {
    options
        .iter()
        .map(|(v, l)| (v.to_string(), l.to_string()))
        .collect()
}

impl Line {
    /// What search scrolls to: the anchor, else the title.
    pub fn anchor_or_title(&self) -> &str {
        self.anchor.as_deref().unwrap_or(&self.title)
    }
}

/// What a click on a control asks of its page: the action, and the value
/// for a switch (`"on"`/`"off"`, the new state) or a choice (the option).
pub fn act_of(control: &Control, option: Option<usize>) -> Option<(String, String)> {
    match control {
        Control::Switch { action, on } => {
            Some((action.clone(), if *on { "off" } else { "on" }.to_string()))
        }
        Control::Choice {
            action, options, ..
        }
        | Control::Pick {
            action, options, ..
        } => {
            let (v, _) = options.get(option?)?;
            Some((action.clone(), v.clone()))
        }
        Control::Button { action, .. } => Some((action.clone(), String::new())),
        Control::Buttons(pills) => {
            let p = pills.get(option?)?;
            p.enabled.then(|| (p.action.clone(), String::new()))
        }
        Control::None | Control::Value(_) | Control::Field { .. } | Control::Check => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sections_in_order() {
        let mut f = Form::default();
        f.section("Look").switch("Dark", None, "dark", true).choice(
            "Tabs",
            Some("where"),
            "tabs",
            &[("left", "Left"), ("top", "Top")],
            "top",
        );
        f.section("About").value("Version", None, "0.1.0");
        assert_eq!(f.sections.len(), 2);
        assert_eq!(f.sections[0].lines.len(), 2);
        assert_eq!(
            f.sections[1].lines[0].control,
            Control::Value("0.1.0".into())
        );
    }

    #[test]
    fn a_switch_asks_for_its_other_state() {
        let c = Control::Switch {
            action: "x".into(),
            on: true,
        };
        assert_eq!(act_of(&c, None), Some(("x".into(), "off".into())));
    }

    #[test]
    fn a_choice_asks_for_the_option_clicked() {
        let c = Control::Choice {
            action: "look".into(),
            options: vec![
                ("light".into(), "Light".into()),
                ("dark".into(), "Dark".into()),
            ],
            chosen: "light".into(),
        };
        assert_eq!(act_of(&c, Some(1)), Some(("look".into(), "dark".into())));
        assert_eq!(act_of(&c, Some(5)), None);
        assert_eq!(act_of(&Control::None, None), None);
    }

    #[test]
    fn line_options_land_on_the_last_line() {
        let mut f = Form::default();
        f.section("Copper on this PC")
            .button("Open links", None, "default", "Make default", true)
            .anchor("general.default")
            .keywords(&["default browser"])
            .disabled(true)
            .tone(Tone::Error)
            .footnote("Kept on this PC");
        f.more().secret("Key", None, "key", "", "sk-…").mono();
        let l = &f.sections[0].lines[0];
        assert_eq!(l.anchor_or_title(), "general.default");
        assert_eq!(l.keywords, vec!["default browser".to_string()]);
        assert!(l.disabled);
        assert_eq!(l.tone, Tone::Error);
        assert_eq!(f.sections[0].note.as_deref(), Some("Kept on this PC"));
        assert!(f.sections[1].continues);
        assert!(matches!(
            f.sections[1].lines[0].control,
            Control::Field {
                secret: true,
                mono: true,
                ..
            }
        ));
    }

    #[test]
    fn a_disabled_pill_asks_nothing() {
        let c = Control::Buttons(vec![
            Pill {
                action: "a".into(),
                label: "A".into(),
                filled: false,
                enabled: true,
            },
            Pill {
                action: "b".into(),
                label: "B".into(),
                filled: true,
                enabled: false,
            },
        ]);
        assert_eq!(act_of(&c, Some(0)), Some(("a".into(), String::new())));
        assert_eq!(act_of(&c, Some(1)), None);
    }
}
