//! Canvas's words and the small decisions behind them, free of the window
//! and the shell so they are tested everywhere. The Mac's copy
//! (`Fork/Canvas/*.swift`) verbatim, with "this Mac" said as "this
//! computer".

use copper_cloud::Failure;
use serde_json::Value;

/// Where a canvas that never leaves the device lives, in a sentence.
pub const PLACE: &str = "this computer";

/// The name a canvas gets when it is given none.
pub const UNTITLED: &str = "Untitled canvas";

/// A refusal the Share card makes itself on a cloud without invite links.
pub const LINKS_UNSUPPORTED: &str = "links_unsupported";

/// The quiet line for an invite that went while it was on screen.
pub const CLOSED_INVITE: &str = "That invite is no longer open";

/// The door's shortcut, as the tooltip shows it.
pub const DOOR_SHORTCUT: &str = "Ctrl+Shift+O";

/// A canvas name as kept: whitespace folded, at most 80 characters
/// (`Canvases.clean`), never empty.
pub fn clean(name: &str) -> String {
    let t: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = t.chars().take(80).collect();
    if t.is_empty() { UNTITLED.into() } else { t }
}

/// Whether a typed name may be submitted (Create / Rename stay disabled).
pub fn submittable(name: &str) -> bool {
    !name.trim().is_empty()
}

/// "Canvas · <name>": the address pill on a canvas tab.
pub fn address_label(name: &str) -> String {
    format!("Canvas · {name}")
}

/// "1 collaborator live", "3 collaborators live" — people, not tabs.
pub fn live(count: usize) -> String {
    format!("{count} collaborator{} live", plural(count))
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// What a canvas row knows about its canvas, to say where it stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowFacts {
    pub personal: bool,
    pub shared: bool,
    pub owner: bool,
    /// Shared, but its account isn't the one signed in: an offline copy.
    pub away: bool,
    /// Signed in on a linked cloud.
    pub cloud_ready: bool,
    /// Settings › Cloud syncs the Personal canvas.
    pub personal_syncs: bool,
    /// Its page is open with a room.
    pub open: bool,
    /// Its room has answered (SyncStep2).
    pub synced: bool,
    pub members: i64,
}

/// The row's second line (`CanvasRow.detail`). `owner` is the owner's name
/// for a canvas shared with you.
pub fn row_detail(f: &RowFacts, owner: Option<&str>) -> String {
    if f.personal {
        if !(f.cloud_ready && f.personal_syncs) {
            return format!("Private · on {PLACE}");
        }
        if f.synced {
            return "Private · synced".into();
        }
        if f.open {
            return "Private · connecting…".into();
        }
        return "Private · sync on".into();
    }
    if f.shared {
        if f.away {
            return format!("Offline · changes saved on {PLACE}");
        }
        if f.open && !f.synced {
            return if f.cloud_ready {
                "Shared · connecting…".into()
            } else {
                format!("Offline · changes saved on {PLACE}")
            };
        }
        if !f.owner
            && let Some(o) = owner.filter(|o| !o.is_empty())
        {
            return format!("From {o}");
        }
        let m = f.members.max(1);
        return if m > 1 {
            format!("Shared · {m} members")
        } else {
            "Shared · just you so far".into()
        };
    }
    format!("On {PLACE}")
}

/// The door's dot: a mention of you is the accent, live people green, an
/// invite alone the column's ink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Mention,
    Live,
    Invite,
}

pub fn door_dot(mentions: usize, others: usize, invites: usize) -> Option<Dot> {
    if mentions > 0 {
        Some(Dot::Mention)
    } else if others > 0 {
        Some(Dot::Live)
    } else if invites > 0 {
        Some(Dot::Invite)
    } else {
        None
    }
}

/// The door's tooltip: "Canvas · 1 invitation · 1 collaborator live   Ctrl+Shift+O".
pub fn door_help(mentions: usize, invites: usize, others: usize) -> String {
    let mut parts = vec!["Canvas".to_string()];
    if mentions > 0 {
        parts.push(format!("{mentions} mention{} of you", plural(mentions)));
    }
    if invites > 0 {
        parts.push(format!("{invites} invitation{}", plural(invites)));
    }
    if others > 0 {
        parts.push(live(others));
    }
    format!("{}   {DOOR_SHORTCUT}", parts.join(" · "))
}

/// The door's accessible name.
pub fn door_name(mentions: usize, invites: usize) -> String {
    if mentions > 0 {
        format!("Canvas, {mentions} mention{} of you", plural(mentions))
    } else if invites > 0 {
        format!("Canvas, {invites} invitation{}", plural(invites))
    } else {
        "Canvas".into()
    }
}

/// The card's header line: the account, or the device.
pub fn header_status(account: Option<&str>) -> String {
    match account.filter(|a| !a.is_empty()) {
        Some(a) => a.to_string(),
        None => format!("On {PLACE}"),
    }
}

/// Under the footer while sharing is out of reach.
pub const FOOTER_CLOUD: &str = "Share canvases and see each other live: ";
pub const FOOTER_SETTINGS: &str = "Settings › Cloud";

/// The New canvas button's tooltip.
pub fn new_help(cloud_ready: bool) -> String {
    if cloud_ready {
        "A shared canvas on your cloud — invite people to it".into()
    } else {
        format!("A canvas on {PLACE}")
    }
}

/// The new form's line under its field.
pub fn new_note(cloud_ready: bool) -> String {
    if cloud_ready {
        "Shared on your cloud — invite people once it's made.".into()
    } else {
        "On this computer. Sign in at Settings › Cloud to share.".into()
    }
}

/// The Signed out section's explanation: the offline copies (nobody
/// signed in), or only their count (another account signed in).
pub fn signed_out_note(count: usize, someone_signed_in: bool) -> String {
    if !someone_signed_in {
        return format!(
            "Kept on {PLACE}. They open offline and keep your changes; sign in at Settings › Cloud to see others' changes and send yours."
        );
    }
    if count == 1 {
        format!(
            "1 shared canvas is kept on {PLACE} for another account. Sign in as that account to open it."
        )
    } else {
        format!(
            "{count} shared canvases are kept on {PLACE} for another account. Sign in as that account to open them."
        )
    }
}

/// What deleting or leaving costs (`CanvasDeleteConfirm.consequence`).
pub fn consequence(leaving: bool, shared: bool) -> String {
    if leaving {
        return "It stays for everyone else. Someone will have to invite you again.".into();
    }
    if shared {
        return "Permanently deletes this canvas and everything on it, for every member. This can't be undone.".into();
    }
    format!(
        "Permanently deletes this canvas and everything on it from {PLACE}. This can't be undone."
    )
}

// MARK: invites

/// The pill's sentence, a long name cut at 48 (`CanvasInvites.line`).
pub fn invite_pill(from: &str, canvas: &str, reminded: bool) -> String {
    let name = if canvas.chars().count() > 48 {
        let mut s: String = canvas.chars().take(46).collect();
        s.push('…');
        s
    } else {
        canvas.to_string()
    };
    match (from.is_empty(), reminded) {
        (true, true) => format!("A reminder: you were invited to “{name}”"),
        (true, false) => format!("You were invited to “{name}”"),
        (false, true) => format!("{from} reminded you about “{name}”"),
        (false, false) => format!("{from} invited you to “{name}”"),
    }
}

/// The Invitations row's second line.
pub fn invite_detail(from: &str, reminded: bool) -> String {
    let who = if from.is_empty() {
        "Invited you".to_string()
    } else {
        format!("From {from}")
    };
    if reminded {
        format!("{who} · reminded you")
    } else {
        who
    }
}

// MARK: mentions

/// A mention of you, as `GET /v1/mentions` lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    pub id: String,
    /// The server's canvas id (a shared canvas's entry id too).
    pub canvas_id: String,
    pub canvas_name: String,
    pub from_id: String,
    pub from_name: String,
    pub from_email: String,
    pub message_id: String,
    pub excerpt: String,
    pub created_at: Option<String>,
}

impl Mention {
    pub fn who(&self) -> &str {
        if !self.from_name.is_empty() {
            &self.from_name
        } else if !self.from_email.is_empty() {
            &self.from_email
        } else {
            "Someone"
        }
    }

    /// Someone put you on a checklist row, not a chat message.
    pub fn is_assignment(&self) -> bool {
        self.message_id.starts_with("assign.")
    }

    pub fn verb(&self) -> &'static str {
        if self.is_assignment() {
            "assigned you"
        } else {
            "mentioned you"
        }
    }

    /// "<who> mentioned you in “<canvas>”" (the pill's first line, after the name).
    pub fn line(&self) -> String {
        format!(" {} in “{}”", self.verb(), self.canvas_name)
    }

    /// One row of `GET /v1/mentions` (`CanvasChat.mention`); read ones and
    /// rows without ids are not mentions waiting.
    pub fn parse(row: &Value) -> Option<Mention> {
        let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
        let id = s(row.get("id")).filter(|v| !v.is_empty())?;
        let message = s(row.get("message_id")).filter(|v| !v.is_empty())?;
        if row.get("read_at").is_some_and(|v| !v.is_null()) {
            return None;
        }
        let canvas = row.get("canvas");
        let from = row.get("from");
        Some(Mention {
            id,
            canvas_id: s(canvas.and_then(|c| c.get("id")))
                .or_else(|| s(row.get("canvas_id")))
                .unwrap_or_default()
                .to_lowercase(),
            canvas_name: s(canvas.and_then(|c| c.get("name"))).unwrap_or_else(|| "a canvas".into()),
            from_id: s(from.and_then(|f| f.get("id")))
                .unwrap_or_default()
                .to_lowercase(),
            from_name: s(from.and_then(|f| f.get("display_name")))
                .or_else(|| s(from.and_then(|f| f.get("name"))))
                .unwrap_or_default(),
            from_email: s(from.and_then(|f| f.get("email"))).unwrap_or_default(),
            message_id: message,
            excerpt: s(row.get("excerpt")).unwrap_or_default(),
            created_at: s(row.get("created_at")),
        })
    }
}

/// Initials for a face: "Ada Lovelace" → "AL", "ada" → "A".
pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name
        .split(|c: char| " ._-@".contains(c))
        .filter(|p| !p.is_empty())
        .collect();
    let first = |p: &str| {
        p.chars()
            .next()
            .map(|c| c.to_uppercase().collect::<String>())
    };
    match parts.as_slice() {
        [] => "?".into(),
        [one] => first(one).unwrap_or_else(|| "?".into()),
        [a, .., b] => format!(
            "{}{}",
            first(a).unwrap_or_default(),
            first(b).unwrap_or_default()
        ),
    }
}

/// `#rrggbb` → `0xrrggbb` (grey when it isn't one).
pub fn hex(text: &str) -> u32 {
    u32::from_str_radix(text.trim().trim_start_matches('#'), 16).unwrap_or(0x888888) & 0xFF_FFFF
}

// MARK: share

/// "Only you here" with the canvas open here and nobody else in it; "3
/// here now" counting you; nothing when it isn't open and nobody is in it.
pub fn presence_line(others: usize, here: bool) -> Option<String> {
    if here {
        return Some(if others == 0 {
            "Only you here".into()
        } else {
            format!("{} here now", others + 1)
        });
    }
    (others > 0).then(|| format!("{others} here now"))
}

/// The linked cloud as people read it — the default `:443` left off.
pub fn cloud_host(host: Option<&str>) -> String {
    match host {
        Some(h) if !h.is_empty() => h.strip_suffix(":443").unwrap_or(h).to_string(),
        _ => "your Copper Cloud".into(),
    }
}

/// The Share card's line on a cloud without share links.
pub fn links_unsupported(host: &str, version: Option<&str>) -> String {
    let runs = match version {
        Some(v) => format!("runs {v}"),
        None => "runs an older version".into(),
    };
    format!(
        "Invite links need Copper Cloud 0.3.0 — {host} {runs}. You can still invite people by email."
    )
}

/// The two forms of a canvas's invite link.
pub fn share_links(token: &str, host: &str, name: &str) -> (String, String) {
    let q = |t: &str| url::form_urlencoded::byte_serialize(t.as_bytes()).collect::<String>();
    (
        format!(
            "copper://canvas/join/{token}?cloud={}&n={}",
            q(host),
            q(name)
        ),
        format!("https://{host}/join/{token}"),
    )
}

/// An address, near enough to send an invite to.
pub fn is_email(text: &str) -> bool {
    let t = text.trim();
    let Some((user, domain)) = t.split_once('@') else {
        return false;
    };
    !user.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !t.chars().any(char::is_whitespace)
        && !domain.contains('@')
}

/// What an invite (or a Resend) came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// A new invite was made.
    pub made: bool,
    /// `Some(true)`: reminded; `Some(false)`: too soon to remind; `None`:
    /// already invited (an older cloud, which doesn't remind).
    pub nudged: Option<bool>,
    /// The invitee's name, if they have an account.
    pub invitee: Option<String>,
    /// The cloud says whether the invitee has an account (0.5.0).
    pub invitee_known: bool,
    pub nudged_before: bool,
}

/// The Share card's line under the field once an invite has been answered
/// (`Canvases.sheetLine`). `nobody`: the directory looked the address up
/// and found no account.
pub fn sheet_line(o: &Outcome, email: &str, name: &str, nobody: bool, host: &str) -> String {
    let who = match &o.invitee {
        Some(n) if !n.is_empty() => n.as_str(),
        _ => name,
    };
    let no_account = if o.invitee_known {
        o.invitee.is_none()
    } else {
        nobody
    };
    if o.made {
        if no_account {
            return format!("Invited {email} — waiting for them to make an account on {host}");
        }
        return format!("Invited {who} — it's waiting for them in Copper");
    }
    match o.nudged {
        Some(true) => format!("Reminded {who}"),
        Some(false) => {
            if no_account {
                format!("{email} has no account on {host} yet — the invite is waiting for them")
            } else if !o.nudged_before {
                format!("Invited {who} moments ago — you can remind them in a minute")
            } else {
                format!("Reminded {who} moments ago — you can remind them again in a minute")
            }
        }
        None => {
            if no_account {
                format!(
                    "{email} is already invited — waiting for them to make an account on {host}"
                )
            } else {
                format!("{who} is already invited — it's waiting for them in Copper")
            }
        }
    }
}

/// What the card was doing when something went wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Doing {
    Inviting(String),
    Reminding(String),
    Withdrawing(String),
    Linking,
    Resetting,
    Removing(String),
    Joining,
    Other,
}

/// One plain sentence for a sharing failure — never the server's short
/// code (`Canvases.plain`).
pub fn plain(f: &Failure, doing: &Doing, host: &str) -> String {
    let fallback = match doing {
        Doing::Inviting(e) => format!("Couldn't invite {e} — try again."),
        Doing::Reminding(n) => format!("Couldn't remind {n} — try again."),
        Doing::Withdrawing(n) => format!("Couldn't withdraw the invite to {n} — try again."),
        Doing::Linking => "Couldn't make an invite link — try again.".into(),
        Doing::Resetting => "Couldn't reset the link — try again.".into(),
        Doing::Removing(n) => format!("Couldn't remove {n} — try again."),
        Doing::Joining => "Couldn't open that invite — try again.".into(),
        Doing::Other => sentence(&f.message),
    };
    let said = f.message.to_lowercase();
    let not_email = || match doing {
        Doing::Inviting(e) => format!("“{e}” isn't an email address."),
        _ => "That isn't an email address.".into(),
    };
    let code = f.code.as_str();
    let st = f.status;
    if code == LINKS_UNSUPPORTED {
        return f.message.clone();
    }
    if code == "not_signed_in" || code == "not_linked" || st == 401 {
        return "Sign in at Settings › Cloud to share canvases.".into();
    }
    if code == "local" || code == "missing" {
        return sentence(&f.message);
    }
    if code == "personal" {
        return "The Personal canvas can't be shared.".into();
    }
    if code == "email" {
        return not_email();
    }
    if st == 400 {
        if said.contains("email") {
            return not_email();
        }
        if said.contains("already a member") {
            return "That's your own address — you're already on this canvas.".into();
        }
        if said.contains("personal") {
            return "The Personal canvas can't be shared.".into();
        }
        return fallback;
    }
    if st == 409 || code == "conflict" {
        return conflict(doing);
    }
    if st == 403 {
        return match doing {
            Doing::Inviting(_) | Doing::Reminding(_) => {
                "Only members of this canvas can invite people to it.".into()
            }
            Doing::Withdrawing(_) => {
                "Only the canvas's owner or whoever sent an invite can withdraw it.".into()
            }
            Doing::Linking => "Only members of this canvas can make an invite link.".into(),
            Doing::Resetting => "Only the canvas's owner can reset its link.".into(),
            Doing::Removing(_) => "Only the canvas's owner can remove people.".into(),
            Doing::Joining => format!("{host} won't open that canvas for this account."),
            Doing::Other => fallback,
        };
    }
    if st == 404 {
        return match doing {
            Doing::Joining => "This invite link no longer works — ask for a new one.".into(),
            Doing::Withdrawing(_) => "That invite was already answered or withdrawn.".into(),
            _ => format!("This canvas isn't on {host} any more."),
        };
    }
    if st == 429 || code == "rate_limited" {
        return "Too many tries — wait a minute and try again.".into();
    }
    if st == 0 && (code == "fingerprint" || code == "tls" || code == "untrusted") {
        return format!(
            "{host}'s certificate isn't the one {PLACE} trusts — check Settings › Cloud."
        );
    }
    if st == 0 && (code == "network" || f.is_network()) {
        return format!("Can't reach {host} right now — check the connection and try again.");
    }
    if st >= 500 {
        return format!("{host} had a problem — try again in a moment.");
    }
    fallback
}

fn conflict(doing: &Doing) -> String {
    match doing {
        Doing::Inviting(e) => format!("{e} is already a member of this canvas."),
        Doing::Reminding(n) => format!("{n} has already joined this canvas."),
        _ => "That changed somewhere else first — try again.".into(),
    }
}

/// A message ended as a sentence.
pub fn sentence(text: &str) -> String {
    let t = text.trim();
    match t.chars().last() {
        Some(c) if !".!?".contains(c) => format!("{t}."),
        _ => t.to_string(),
    }
}

/// The Share card when the canvas can't be shared.
pub fn unshareable(personal: bool) -> String {
    if personal {
        "Personal canvases can't be shared.".into()
    } else {
        format!("This canvas is on {PLACE} only and can't be shared.")
    }
}

/// The Invited label's help: what an invite is waiting for.
pub fn waiting_help(no_account: bool, host: &str) -> String {
    if no_account {
        format!("Waiting for them to make an account on {host}")
    } else {
        "Waiting for them to join — it's in their Copper".into()
    }
}

/// A role as a word: `owner` → "Owner".
pub fn role_word(role: &str) -> String {
    let mut c = role.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "Member".into(),
    }
}

/// The page for an address that names no canvas here (instead of the
/// engine's error page).
pub fn missing_page(id: &str) -> String {
    let esc = id
        .chars()
        .take(120)
        .map(|c| match c {
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '&' => "&amp;".to_string(),
            '"' => "&quot;".to_string(),
            c => c.to_string(),
        })
        .collect::<String>();
    format!(
        "<!doctype html><meta charset=utf-8><meta name=color-scheme content='light dark'><title>No such canvas</title>\
         <body style='font:14px system-ui,sans-serif;margin:0;display:grid;place-items:center;height:100vh'>\
         <div style='max-width:420px;text-align:center'><h1 style='font-size:17px;font-weight:600'>There's no canvas here</h1>\
         <p style='opacity:.7'>“{esc}” isn't one of your canvases on {PLACE}. It may have been deleted, or it's shared on an account that isn't signed in.</p>\
         <p style='opacity:.7'>Open the Canvas card at the foot of the sidebar to see your canvases.</p></div></body>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_are_cleaned_like_the_mac() {
        assert_eq!(clean("  a   b \n c "), "a b c");
        assert_eq!(clean("   "), UNTITLED);
        assert_eq!(clean(&"x".repeat(200)).chars().count(), 80);
        assert!(!submittable("  "));
        assert!(submittable(" a "));
    }

    #[test]
    fn rows_say_where_a_canvas_stands() {
        let personal = RowFacts {
            personal: true,
            ..Default::default()
        };
        assert_eq!(row_detail(&personal, None), "Private · on this computer");
        let syncing = RowFacts {
            cloud_ready: true,
            personal_syncs: true,
            ..personal
        };
        assert_eq!(row_detail(&syncing, None), "Private · sync on");
        assert_eq!(
            row_detail(
                &RowFacts {
                    open: true,
                    ..syncing
                },
                None
            ),
            "Private · connecting…"
        );
        assert_eq!(
            row_detail(
                &RowFacts {
                    open: true,
                    synced: true,
                    ..syncing
                },
                None
            ),
            "Private · synced"
        );
        let shared = RowFacts {
            shared: true,
            owner: true,
            cloud_ready: true,
            members: 1,
            ..Default::default()
        };
        assert_eq!(row_detail(&shared, None), "Shared · just you so far");
        assert_eq!(
            row_detail(
                &RowFacts {
                    members: 3,
                    ..shared
                },
                None
            ),
            "Shared · 3 members"
        );
        assert_eq!(
            row_detail(
                &RowFacts {
                    owner: false,
                    ..shared
                },
                Some("Ada")
            ),
            "From Ada"
        );
        assert_eq!(
            row_detail(
                &RowFacts {
                    away: true,
                    ..shared
                },
                None
            ),
            "Offline · changes saved on this computer"
        );
        assert_eq!(
            row_detail(
                &RowFacts {
                    open: true,
                    ..shared
                },
                None
            ),
            "Shared · connecting…"
        );
        assert_eq!(row_detail(&RowFacts::default(), None), "On this computer");
    }

    #[test]
    fn no_mac_in_windows_copy() {
        let all = [
            row_detail(&RowFacts::default(), None),
            signed_out_note(1, false),
            signed_out_note(1, true),
            signed_out_note(2, true),
            consequence(false, false),
            new_note(false),
            new_help(false),
            unshareable(false),
            header_status(None),
        ];
        for s in all {
            assert!(!s.contains("Mac"), "{s}");
        }
        assert!(signed_out_note(2, true).starts_with("2 shared canvases are kept"));
        assert!(signed_out_note(1, true).ends_with("to open it."));
    }

    #[test]
    fn the_door_says_what_waits() {
        assert_eq!(door_dot(1, 1, 1), Some(Dot::Mention));
        assert_eq!(door_dot(0, 2, 1), Some(Dot::Live));
        assert_eq!(door_dot(0, 0, 1), Some(Dot::Invite));
        assert_eq!(door_dot(0, 0, 0), None);
        assert_eq!(
            door_help(0, 1, 1),
            "Canvas · 1 invitation · 1 collaborator live   Ctrl+Shift+O"
        );
        assert_eq!(
            door_help(2, 0, 0),
            "Canvas · 2 mentions of you   Ctrl+Shift+O"
        );
        assert_eq!(door_name(0, 2), "Canvas, 2 invitations");
    }

    #[test]
    fn invite_pills_cut_long_names() {
        assert_eq!(
            invite_pill("Ada", "Plan", false),
            "Ada invited you to “Plan”"
        );
        assert_eq!(
            invite_pill("Ada", "Plan", true),
            "Ada reminded you about “Plan”"
        );
        assert_eq!(invite_pill("", "Plan", false), "You were invited to “Plan”");
        assert_eq!(
            invite_pill("", "Plan", true),
            "A reminder: you were invited to “Plan”"
        );
        let long = "n".repeat(60);
        let line = invite_pill("A", &long, false);
        assert!(line.contains(&format!("{}…", "n".repeat(46))));
        assert_eq!(invite_detail("", false), "Invited you");
        assert_eq!(invite_detail("Ada", true), "From Ada · reminded you");
    }

    #[test]
    fn mentions_parse_and_read() {
        let row = json!({"id": "m1", "message_id": "assign.s1.n", "canvas": {"id": "ABC", "name": "Board"},
            "from": {"id": "U1", "display_name": "Ada Lovelace"}, "excerpt": "row"});
        let m = Mention::parse(&row).unwrap();
        assert_eq!(m.canvas_id, "abc");
        assert!(m.is_assignment());
        assert_eq!(m.verb(), "assigned you");
        assert_eq!(m.who(), "Ada Lovelace");
        assert_eq!(m.line(), " assigned you in “Board”");
        assert!(Mention::parse(&json!({"id": "m", "message_id": "x", "read_at": "t"})).is_none());
        assert!(Mention::parse(&json!({"id": "m"})).is_none());
        let bare =
            Mention::parse(&json!({"id": "m", "message_id": "x", "canvas_id": "C"})).unwrap();
        assert_eq!(bare.who(), "Someone");
        assert_eq!(bare.canvas_name, "a canvas");
        assert_eq!(bare.canvas_id, "c");
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("ada@x.io"), "AI");
        assert_eq!(initials("bob"), "B");
        assert_eq!(initials(""), "?");
        assert_eq!(hex("#30A46C"), 0x30A46C);
        assert_eq!(hex("zz"), 0x888888);
    }

    #[test]
    fn share_words() {
        assert_eq!(presence_line(0, true).as_deref(), Some("Only you here"));
        assert_eq!(presence_line(2, true).as_deref(), Some("3 here now"));
        assert_eq!(presence_line(2, false).as_deref(), Some("2 here now"));
        assert_eq!(presence_line(0, false), None);
        assert_eq!(cloud_host(Some("c.example:443")), "c.example");
        assert_eq!(cloud_host(None), "your Copper Cloud");
        assert_eq!(
            links_unsupported("c.example", Some("0.2.0")),
            "Invite links need Copper Cloud 0.3.0 — c.example runs 0.2.0. You can still invite people by email."
        );
        let (link, web) = share_links("tok", "c.example:8443", "My board");
        assert_eq!(
            link,
            "copper://canvas/join/tok?cloud=c.example%3A8443&n=My+board"
        );
        assert_eq!(web, "https://c.example:8443/join/tok");
        assert!(is_email("a@b.co"));
        assert!(!is_email("a@b"));
        assert!(!is_email("a b@c.de"));
        assert!(!is_email("@c.de"));
    }

    #[test]
    fn invite_outcomes_in_words() {
        let made = Outcome {
            made: true,
            invitee: Some("Bo".into()),
            invitee_known: true,
            ..Default::default()
        };
        assert_eq!(
            sheet_line(&made, "b@x.io", "b@x.io", false, "h"),
            "Invited Bo — it's waiting for them in Copper"
        );
        let nobody = Outcome {
            made: true,
            invitee_known: true,
            ..Default::default()
        };
        assert_eq!(
            sheet_line(&nobody, "b@x.io", "b@x.io", false, "h"),
            "Invited b@x.io — waiting for them to make an account on h"
        );
        let again = Outcome {
            nudged: Some(true),
            ..Default::default()
        };
        assert_eq!(sheet_line(&again, "e", "Bo", false, "h"), "Reminded Bo");
        let soon = Outcome {
            nudged: Some(false),
            ..Default::default()
        };
        assert_eq!(
            sheet_line(&soon, "e", "Bo", false, "h"),
            "Invited Bo moments ago — you can remind them in a minute"
        );
    }

    #[test]
    fn failures_in_words() {
        let f = |s: u16, c: &str, m: &str| Failure::new(s, c, m);
        let inv = Doing::Inviting("b@x.io".into());
        assert_eq!(
            plain(&f(400, "bad_request", "invalid email"), &inv, "h"),
            "“b@x.io” isn't an email address."
        );
        assert_eq!(
            plain(&f(400, "bad_request", "already a member"), &inv, "h"),
            "That's your own address — you're already on this canvas."
        );
        assert_eq!(
            plain(&f(409, "conflict", "x"), &inv, "h"),
            "b@x.io is already a member of this canvas."
        );
        assert_eq!(
            plain(&f(404, "not_found", "x"), &Doing::Joining, "h"),
            "This invite link no longer works — ask for a new one."
        );
        assert_eq!(
            plain(&f(503, "x", "x"), &Doing::Linking, "h"),
            "h had a problem — try again in a moment."
        );
        assert_eq!(
            plain(&f(401, "x", "x"), &Doing::Linking, "h"),
            "Sign in at Settings › Cloud to share canvases."
        );
        assert_eq!(sentence("done"), "done.");
        assert_eq!(sentence("done!"), "done!");
    }

    #[test]
    fn the_missing_page_escapes_the_id() {
        let p = missing_page("<b>");
        assert!(p.contains("&lt;b&gt;"));
        assert!(!p.contains("<b>"));
        assert_eq!(address_label("Plan"), "Canvas · Plan");
        assert_eq!(role_word("owner"), "Owner");
    }
}
