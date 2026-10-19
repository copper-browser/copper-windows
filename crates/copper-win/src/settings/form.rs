//! What a Settings page says, as plain data: sections of lines, each with
//! one control. A page describes itself from what the shell knows
//! ([`super::Page::draw`]); the panel turns the description into nodes and
//! hands clicks back as the control's action ([`super::Page::act`]). Pages
//! never touch the window, so they cannot trip over its borrow, and adding
//! one is only words and actions.

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
}

/// One thing to set or do (`Line`): words on the left, a control on the
/// right.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub title: String,
    pub detail: Option<String>,
    pub control: Control,
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
}

impl Form {
    /// Start a section titled `title`.
    pub fn section(&mut self, title: &str) -> &mut Section {
        self.sections.push(Section {
            title: title.to_string(),
            lines: Vec::new(),
        });
        self.sections.last_mut().expect("just pushed")
    }
}

impl Section {
    fn push(&mut self, title: &str, detail: Option<&str>, control: Control) -> &mut Section {
        self.lines.push(Line {
            title: title.to_string(),
            detail: detail.map(str::to_string),
            control,
        });
        self
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
                options: options
                    .iter()
                    .map(|(v, l)| (v.to_string(), l.to_string()))
                    .collect(),
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
        } => {
            let (v, _) = options.get(option?)?;
            Some((action.clone(), v.clone()))
        }
        Control::Button { action, .. } => Some((action.clone(), String::new())),
        Control::None | Control::Value(_) => None,
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
}
