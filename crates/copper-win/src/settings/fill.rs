//! Form fill's words and geometry, kept apart from the widgets so host
//! tests can check them: the save offer's question and buttons
//! (`App.swift` `keepAsking`, `Browser.swift` `keepOffer`), and the
//! account list's rows, footer and place under its box (`Accounts.swift`,
//! `Fork/Credentials/PasswordVaults.swift` `PickerFooter`).
//!
//! Windows says Credential Manager where the Mac says "your keychain".

use copper_creds::Source;
use copper_shell_core::forms::view::{Row, RowKind};

/// Copper's own store, as the offer and the list name it.
pub const OWN: &str = "Credential Manager";

/// A row's height (`Accounts.swift`: two lines in 7-pt padding).
pub const ROW_H: f32 = 44.0;
/// Rows shown before the rest scroll.
pub const ROWS_SHOWN: usize = 6;
/// The list's most, then it scrolls: six rows and a peek (`6 × 44 + 22`).
pub const LIST_MAX: f32 = ROWS_SHOWN as f32 * ROW_H + 22.0;
/// The footer's height (10.5-pt line in 7-pt padding).
pub const FOOT_H: f32 = 28.0;
/// The gap between the box and the list.
pub const GAP: f32 = 6.0;
/// Room kept from the page's edges when the list must be moved in.
pub const EDGE: f32 = 8.0;

/// A vault's name in the offer and the list.
pub fn title(s: Source) -> &'static str {
    match s {
        Source::Keychain => OWN,
        Source::Bitwarden => "Bitwarden",
        Source::OnePassword => "1Password",
    }
}

/// The offer's question and its primary button, in the Mac's words.
pub fn question(user: &str, host: &str, target: Source, changed: bool) -> (String, String) {
    let vault = target != Source::Keychain;
    let name = title(target);
    let text = match (vault, changed) {
        (true, true) => format!("Update the password in {name} for {user} on {host}?"),
        (true, false) => format!("Save to {name} for {user} on {host}?"),
        (false, true) => format!("Update the password for {user} on {host}?"),
        (false, false) if user.is_empty() => format!("Save this password for {host}?"),
        (false, false) => format!("Save the password for {user} on {host}?"),
    };
    let button = match (vault, changed) {
        (true, true) => format!("Update in {name}"),
        (true, false) => format!("Save to {name}"),
        (false, true) => "Update".to_string(),
        (false, false) => "Save".to_string(),
    };
    (text, button)
}

/// What the announcement says once a save went in (`keepOffer`).
pub fn saved(host: &str, target: Source, changed: bool) -> String {
    let name = title(target);
    match (target != Source::Keychain, changed) {
        (true, true) => format!("Password updated in {name} for {host}"),
        (true, false) => format!("Password saved to {name}"),
        (false, true) => format!("Password updated for {host}"),
        (false, false) => format!("Password saved for {host}"),
    }
}

/// What the announcement says when a save didn't go in. Credential
/// Manager's refusal is said plainly, as the Mac says the keychain's; a
/// manager's own words are passed on.
pub fn refused(target: Source, why: &str) -> String {
    match target {
        Source::Keychain => format!("{OWN} refused it"),
        _ if why.trim().is_empty() => format!("{} refused it", title(target)),
        _ => why.to_string(),
    }
}

/// A row's two lines: the account, and where it is from.
pub fn row_lines(r: &Row) -> (String, String) {
    let c = &r.credential;
    let user = if c.user.is_empty() {
        "No name".to_string()
    } else {
        c.user.clone()
    };
    let under = match r.kind {
        RowKind::Code => format!("Two-step code · {}", c.host),
        RowKind::Login => c.host.clone(),
    };
    (user, under)
}

/// Which managers stand behind the list but aren't in its rows: locked,
/// or still loading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    pub bitwarden: bool,
    pub onepassword: bool,
}

/// The footer (`PickerFooter.text`): "From Credential Manager", "From
/// Credential Manager and Bitwarden", "From Credential Manager, Bitwarden
/// and 1Password"; a list of codes only from managers says just them.
pub fn footer(rows: &[Row], pending: Pending) -> String {
    let mut bw = false;
    let mut op = false;
    let mut codes_only = !rows.is_empty();
    for r in rows {
        match r.credential.source {
            Source::Bitwarden => bw = true,
            Source::OnePassword => op = true,
            Source::Keychain => {}
        }
        if r.kind != RowKind::Code {
            codes_only = false;
        }
    }
    let names = |bw: bool, op: bool| -> Vec<&'static str> {
        [(bw, "Bitwarden"), (op, "1Password")]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| n)
            .collect()
    };
    if codes_only && (bw || op) && !rows.iter().any(|r| r.credential.source == Source::Keychain) {
        return format!("From {}", names(bw, op).join(" and "));
    }
    let more = names(bw || pending.bitwarden, op || pending.onepassword);
    match more.as_slice() {
        [] => format!("From {OWN}"),
        [one] => format!("From {OWN} and {one}"),
        [a, b] => format!("From {OWN}, {a} and {b}"),
        _ => unreachable!("two managers at most"),
    }
}

/// A rectangle in window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The list's width for a box `field_w` wide: the box's, within 240–360.
pub fn width(field_w: f32) -> f32 {
    field_w.clamp(240.0, 360.0)
}

/// The list's height with `rows` rows: every row up to six, then
/// [`LIST_MAX`] and a scroll, and the footer.
pub fn height(rows: usize) -> f32 {
    let list = if rows > ROWS_SHOWN {
        LIST_MAX
    } else {
        rows as f32 * ROW_H
    };
    list + FOOT_H
}

/// Where the list goes for the box at `field` (window pixels) on a page
/// at `page`: under the box's left edge at `+6`; above it when there is
/// no room below and more above; moved in from the sides rather than cut.
pub fn place(field: Rect, page: Rect, rows: usize) -> Rect {
    let w = width(field.w);
    let h = height(rows);
    let below = field.y + field.h + GAP;
    let above = field.y - GAP - h;
    let bottom = page.y + page.h - EDGE;
    let room_below = bottom - below;
    let room_above = field.y - GAP - (page.y + EDGE);
    let y = if room_below >= h || room_below >= room_above {
        below
    } else {
        above
    };
    let x = field.x.min(page.x + page.w - EDGE - w).max(page.x + EDGE);
    Rect { x, y, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use copper_creds::{Credential, CredentialId};

    fn row(user: &str, source: Source, kind: RowKind) -> Row {
        Row {
            credential: Credential {
                id: CredentialId::Keychain {
                    host: "example.com".into(),
                    user: user.into(),
                },
                source,
                host: "example.com".into(),
                user: user.into(),
                sites: vec![],
                name: String::new(),
                has_totp: false,
                used: None,
                folder: None,
                agent_hint: Default::default(),
            },
            kind,
        }
    }

    #[test]
    fn the_offer_asks_like_the_mac() {
        let q = |u, t, c| question(u, "example.com", t, c);
        assert_eq!(
            q("", Source::Keychain, false),
            ("Save this password for example.com?".into(), "Save".into())
        );
        assert_eq!(
            q("ana", Source::Keychain, false).0,
            "Save the password for ana on example.com?"
        );
        assert_eq!(
            q("ana", Source::Keychain, true),
            (
                "Update the password for ana on example.com?".into(),
                "Update".into()
            )
        );
        assert_eq!(
            q("ana", Source::Bitwarden, false),
            (
                "Save to Bitwarden for ana on example.com?".into(),
                "Save to Bitwarden".into()
            )
        );
        assert_eq!(
            q("ana", Source::OnePassword, true),
            (
                "Update the password in 1Password for ana on example.com?".into(),
                "Update in 1Password".into()
            )
        );
    }

    #[test]
    fn the_offer_says_what_came_of_it() {
        assert_eq!(
            saved("a.com", Source::Keychain, false),
            "Password saved for a.com"
        );
        assert_eq!(
            saved("a.com", Source::Keychain, true),
            "Password updated for a.com"
        );
        assert_eq!(
            saved("a.com", Source::Bitwarden, false),
            "Password saved to Bitwarden"
        );
        assert_eq!(
            saved("a.com", Source::OnePassword, true),
            "Password updated in 1Password for a.com"
        );
        assert_eq!(
            refused(Source::Keychain, "Element not found"),
            "Credential Manager refused it"
        );
        assert_eq!(refused(Source::Bitwarden, ""), "Bitwarden refused it");
        assert_eq!(
            refused(Source::OnePassword, "Vault is read-only"),
            "Vault is read-only"
        );
    }

    #[test]
    fn rows_read_like_the_mac() {
        assert_eq!(
            row_lines(&row("", Source::Keychain, RowKind::Login)),
            ("No name".into(), "example.com".into())
        );
        assert_eq!(
            row_lines(&row("ana", Source::Bitwarden, RowKind::Code)),
            ("ana".into(), "Two-step code · example.com".into())
        );
    }

    #[test]
    fn the_footer_names_every_store_behind_the_list() {
        let none = Pending::default();
        let kc = row("a", Source::Keychain, RowKind::Login);
        let bw = row("b", Source::Bitwarden, RowKind::Login);
        let op = row("c", Source::OnePassword, RowKind::Login);
        assert_eq!(
            footer(std::slice::from_ref(&kc), none),
            "From Credential Manager"
        );
        assert_eq!(
            footer(&[kc.clone(), bw.clone()], none),
            "From Credential Manager and Bitwarden"
        );
        assert_eq!(
            footer(&[kc.clone(), op.clone(), bw.clone()], none),
            "From Credential Manager, Bitwarden and 1Password"
        );
        assert_eq!(
            footer(
                std::slice::from_ref(&kc),
                Pending {
                    bitwarden: false,
                    onepassword: true
                }
            ),
            "From Credential Manager and 1Password"
        );
        let code = row("c", Source::OnePassword, RowKind::Code);
        assert_eq!(footer(&[code], none), "From 1Password");
    }

    #[test]
    fn the_list_hangs_under_its_box() {
        let page = Rect {
            x: 0.0,
            y: 80.0,
            w: 1200.0,
            h: 800.0,
        };
        let field = Rect {
            x: 100.0,
            y: 200.0,
            w: 300.0,
            h: 30.0,
        };
        let r = place(field, page, 2);
        assert_eq!((r.x, r.y, r.w), (100.0, 236.0, 300.0));
        assert_eq!(r.h, 2.0 * ROW_H + FOOT_H);
        // Narrow and wide boxes: 240–360.
        assert_eq!(place(Rect { w: 120.0, ..field }, page, 1).w, 240.0);
        assert_eq!(place(Rect { w: 900.0, ..field }, page, 1).w, 360.0);
        // Six rows show, then it scrolls.
        assert_eq!(height(9), LIST_MAX + FOOT_H);
        assert_eq!(LIST_MAX, 286.0);
    }

    #[test]
    fn the_list_turns_up_or_in_at_the_edges() {
        let page = Rect {
            x: 0.0,
            y: 80.0,
            w: 1200.0,
            h: 800.0,
        };
        // Near the foot: above the box.
        let low = Rect {
            x: 100.0,
            y: 820.0,
            w: 300.0,
            h: 30.0,
        };
        let r = place(low, page, 3);
        assert_eq!(r.y, 820.0 - GAP - height(3));
        // Near the right edge: moved in, not cut.
        let right = Rect {
            x: 1100.0,
            y: 200.0,
            w: 300.0,
            h: 30.0,
        };
        assert_eq!(place(right, page, 1).x, 1200.0 - EDGE - 300.0);
    }
}
