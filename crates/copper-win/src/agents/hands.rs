//! Who is driving which tab (the Mac's `Hands.swift`; Linux `agents/mark.rs`'s
//! model), as plain state: the drive events go in, what to draw comes out.
//! Drawn by [`super::marks`].
//!
//! * A **hand** is one agent session (`Who.key`) on one tab: what it is
//!   doing, whether a call is in flight, when it last touched the tab.
//! * Its **colour** is picked once per session and kept for the app's
//!   lifetime: the seed's swatch, or the next one no other live hand has.
//! * A tab shows its hands as **badges** (at most three, the latest first)
//!   while busy and for 30 s after; the tab on screen also gets the
//!   **bar** (the same hands, as capsules with Stop) and the **ring** while
//!   one of them is busy.

use copper_core::browser::TabId;
use copper_mcp::Who;
use copper_mcp::drive::{DriveEvent, REFUSAL, Status};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A hand's marks stay this long after its last call (Mac `Hands`).
pub const LINGER: Duration = Duration::from_secs(30);
/// At most this many hands per tab are drawn.
pub const MOST: usize = 3;

/// The Mac's agent colours (`Drive.swatches`), as 0–1 RGB.
pub const SWATCHES: [(f32, f32, f32); 9] = [
    (0.90, 0.28, 0.30),
    (0.93, 0.69, 0.13),
    (0.19, 0.64, 0.42),
    (0.07, 0.65, 0.58),
    (0.02, 0.62, 0.80),
    (0.24, 0.39, 0.87),
    (0.43, 0.34, 0.81),
    (0.56, 0.31, 0.78),
    (0.84, 0.25, 0.62),
];

/// Copper's own (pane) agent keeps Copper's colour.
pub const COPPER: (f32, f32, f32) = (0.85, 0.47, 0.34);

/// One agent's hand on one tab.
#[derive(Clone, Debug, PartialEq)]
pub struct Hand {
    pub key: String,
    pub agent: String,
    pub thread: String,
    /// One or two letters for the badge ("CC", "P"); empty for Jev (a
    /// sparkle is drawn instead).
    pub initials: String,
    pub color: (f32, f32, f32),
    /// What it is doing: the call's words, the Jev goal or phase.
    pub action: String,
    /// Calls in flight.
    pub busy: u32,
    pub last: Instant,
    pub jev: bool,
    pub stopped: bool,
}

impl Hand {
    /// Still drawn at `now`.
    pub fn live(&self, now: Instant) -> bool {
        self.busy > 0 || now.saturating_duration_since(self.last) < LINGER
    }

    /// Stopped (not a Jev run, which just ends) and its calls still turned
    /// away at `now`: "Let it back in" shows (Mac `DriveCard`).
    pub fn refused(&self, now: Instant) -> bool {
        self.stopped && !self.jev && now.saturating_duration_since(self.last) < REFUSAL
    }

    /// The text dark enough on its colour (amber takes dark ink).
    pub fn dark_ink(&self) -> bool {
        self.color == SWATCHES[1]
    }
}

/// The badge's letters for an agent's name.
pub fn initials(agent: &str) -> String {
    let words: Vec<&str> = agent
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    match words.as_slice() {
        [] => "A".into(),
        [one] => one
            .chars()
            .next()
            .map_or("A".into(), |c| c.to_uppercase().collect()),
        [a, b, ..] => [a, b]
            .iter()
            .filter_map(|w| w.chars().next())
            .flat_map(char::to_uppercase)
            .collect(),
    }
}

/// FNV-1a of `seed`, into the swatches.
fn seeded(seed: &str) -> usize {
    let mut hash: u32 = 2_166_136_261;
    for b in seed.bytes() {
        hash = (hash ^ u32::from(b)).wrapping_mul(16_777_619);
    }
    (hash % SWATCHES.len() as u32) as usize
}

/// Every hand on every tab.
#[derive(Debug, Default)]
pub struct Hands {
    tabs: HashMap<TabId, Vec<Hand>>,
    /// A call's ticket → the tab it began on, and whether the call put
    /// the hand there.
    tickets: HashMap<u64, (TabId, bool)>,
    /// The live Jev run's tab.
    run: Option<TabId>,
    /// Each session's colour, for the app's lifetime.
    colors: HashMap<String, (f32, f32, f32)>,
    /// The current space's tint, as a hue (0…1): a new session's colour
    /// keeps clear of it.
    pub tint: Option<f32>,
}

/// How near a swatch's hue may come to the space's tint.
const TINT_CLEAR: f32 = 0.045;

/// The hue of `c`, 0…1 round the wheel.
pub fn hue((r, g, b): (f32, f32, f32)) -> f32 {
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    if d <= f32::EPSILON {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h / 6.0
}

fn near_hue(a: f32, b: f32) -> bool {
    let d = (a - b).abs();
    d.min(1.0 - d) < TINT_CLEAR
}

impl Hands {
    /// The colour of session `who`: kept once given; else its seed's
    /// swatch, or the next one no other live hand wears.
    fn color(&mut self, who: &Who, now: Instant) -> (f32, f32, f32) {
        if who.key == "pane" {
            return COPPER;
        }
        if let Some(c) = self.colors.get(&who.key) {
            return *c;
        }
        let worn: Vec<(f32, f32, f32)> = self
            .tabs
            .values()
            .flatten()
            .filter(|h| h.key != who.key && h.live(now))
            .map(|h| h.color)
            .collect();
        let start = seeded(if who.seed.is_empty() {
            &who.key
        } else {
            &who.seed
        });
        let tint = self.tint;
        let pick = (0..SWATCHES.len())
            .map(|i| SWATCHES[(start + i) % SWATCHES.len()])
            .find(|c| !worn.contains(c) && !tint.is_some_and(|t| near_hue(hue(*c), t)))
            .unwrap_or(SWATCHES[start]);
        self.colors.insert(who.key.clone(), pick);
        pick
    }

    /// One drive event; the tabs whose marks changed.
    pub fn event(&mut self, e: &DriveEvent, now: Instant) -> Vec<TabId> {
        match e {
            DriveEvent::CallBegan {
                ticket,
                who,
                title,
                tab: Some(tab),
                ..
            } => {
                let new = self.touch(tab, who, Some(title), 1, false, now);
                self.tickets.insert(*ticket, (tab.clone(), new));
                vec![tab.clone()]
            }
            DriveEvent::CallEnded {
                ticket, who, tab, ..
            } => {
                let mut changed = Vec::new();
                if let Some((began, new)) = self.tickets.remove(ticket) {
                    if let Some(h) = self.hand_mut(&began, &who.key) {
                        h.busy = h.busy.saturating_sub(1);
                        h.last = now;
                    }
                    // A call that ended up elsewhere (`browser_tabs new`
                    // begins on the user's tab, ends on the one it made)
                    // leaves no mark where it only passed through.
                    if new
                        && tab.as_ref().is_some_and(|t| *t != began)
                        && let Some(hs) = self.tabs.get_mut(&began)
                    {
                        hs.retain(|h| h.key != who.key || h.busy > 0);
                    }
                    changed.push(began);
                }
                if let Some(t) = tab
                    && !changed.contains(t)
                {
                    self.touch(t, who, None, 0, false, now);
                    changed.push(t.clone());
                }
                changed
            }
            DriveEvent::CallRefused { who, .. } => self.tabs_of(&who.key),
            DriveEvent::RunBegan { who, goal, tab } => {
                self.run = Some(tab.clone());
                self.touch(tab, who, Some(goal), 1, true, now);
                vec![tab.clone()]
            }
            DriveEvent::Phase { title, .. } => {
                let Some(tab) = self.run.clone() else {
                    return vec![];
                };
                match self
                    .tabs
                    .get_mut(&tab)
                    .and_then(|hs| hs.iter_mut().find(|h| h.jev && h.busy > 0))
                {
                    Some(h) => {
                        h.action = title.clone();
                        h.last = now;
                        vec![tab]
                    }
                    None => vec![],
                }
            }
            DriveEvent::RunFinished { status, note } => {
                let Some(tab) = self.run.take() else {
                    return vec![];
                };
                if let Some(h) = self
                    .tabs
                    .get_mut(&tab)
                    .and_then(|hs| hs.iter_mut().find(|h| h.jev && h.busy > 0))
                {
                    h.busy = 0;
                    h.last = now;
                    h.action = match status {
                        Status::Done => "Done".into(),
                        Status::Stopped => "Stopped".into(),
                        _ if !note.trim().is_empty() => note.clone(),
                        _ => "Finished".into(),
                    };
                }
                vec![tab]
            }
            _ => vec![],
        }
    }

    fn touch(
        &mut self,
        tab: &TabId,
        who: &Who,
        action: Option<&String>,
        busy: u32,
        jev: bool,
        now: Instant,
    ) -> bool {
        let color = self.color(who, now);
        let hands = self.tabs.entry(tab.clone()).or_default();
        let live = |h: &Hand| h.live(now);
        let fresh = !hands.iter().any(|h| h.key == who.key && live(h));
        match hands.iter_mut().find(|h| h.key == who.key) {
            Some(h) => {
                h.busy += busy;
                h.last = now;
                h.jev |= jev;
                h.stopped = false;
                if let Some(a) = action {
                    h.action = a.clone();
                }
            }
            None => hands.push(Hand {
                key: who.key.clone(),
                agent: who.agent.clone(),
                thread: who.thread.clone(),
                initials: if jev {
                    String::new()
                } else {
                    initials(&who.agent)
                },
                color,
                action: action.cloned().unwrap_or_default(),
                busy,
                last: now,
                jev,
                stopped: false,
            }),
        }
        fresh
    }

    fn hand_mut(&mut self, tab: &TabId, key: &str) -> Option<&mut Hand> {
        self.tabs.get_mut(tab)?.iter_mut().find(|h| h.key == key)
    }

    fn tabs_of(&self, key: &str) -> Vec<TabId> {
        self.tabs
            .iter()
            .filter(|(_, hs)| hs.iter().any(|h| h.key == key))
            .map(|(t, _)| t.clone())
            .collect()
    }

    /// Stop pressed on `key`'s mark: its hands say so (the refusal itself
    /// is the server's, `Agents::stop_hand`).
    pub fn stop(&mut self, key: &str, now: Instant) -> Vec<TabId> {
        for hs in self.tabs.values_mut() {
            for h in hs.iter_mut().filter(|h| h.key == key) {
                h.stopped = true;
                h.busy = 0;
                h.action = "Stopped — its next calls are turned away".into();
                h.last = now;
            }
        }
        self.tabs_of(key)
    }

    /// "Let it back in": every stopped hand is a plain one again (the
    /// server's side is `Agents::resume_hand`); the tabs that changed.
    pub fn let_back_in(&mut self) -> Vec<TabId> {
        let mut tabs = Vec::new();
        for (t, hs) in self.tabs.iter_mut() {
            for h in hs.iter_mut().filter(|h| h.stopped) {
                h.stopped = false;
                h.action.clear();
                tabs.push(t.clone());
            }
        }
        tabs
    }

    /// The server stopped: forget everything; the tabs that had marks.
    pub fn clear(&mut self) -> Vec<TabId> {
        self.tickets.clear();
        self.run = None;
        self.tabs.drain().map(|(t, _)| t).collect()
    }

    /// The tab closed.
    pub fn forget(&mut self, tab: &TabId) {
        self.tabs.remove(tab);
        self.tickets.retain(|_, (t, _)| t != tab);
    }

    /// `tab`'s live hands, the latest first, at most [`MOST`].
    pub fn on(&self, tab: &TabId, now: Instant) -> Vec<Hand> {
        let mut hs: Vec<Hand> = self
            .tabs
            .get(tab)
            .map(|hs| hs.iter().filter(|h| h.live(now)).cloned().collect())
            .unwrap_or_default();
        hs.sort_by(|a, b| b.busy.min(1).cmp(&a.busy.min(1)).then(b.last.cmp(&a.last)));
        hs.truncate(MOST);
        hs
    }

    /// The ring's colour on `tab`: its busiest live hand's, while one is
    /// at work.
    pub fn ring(&self, tab: &TabId, now: Instant) -> Option<(f32, f32, f32)> {
        self.on(tab, now)
            .into_iter()
            .find(|h| h.busy > 0)
            .map(|h| h.color)
    }

    /// A hand at work on a tab other than `shown` (the top-right pill's).
    pub fn elsewhere(&self, shown: Option<&TabId>, now: Instant) -> Option<Hand> {
        self.tabs
            .iter()
            .filter(|(t, _)| Some(*t) != shown)
            .flat_map(|(_, hs)| hs.iter())
            .filter(|h| h.busy > 0 && h.live(now))
            .max_by_key(|h| h.last)
            .cloned()
    }

    /// Every tab with a live hand.
    pub fn tabs(&self, now: Instant) -> Vec<TabId> {
        self.tabs
            .iter()
            .filter(|(_, hs)| hs.iter().any(|h| h.live(now)))
            .map(|(t, _)| t.clone())
            .collect()
    }

    /// When the next mark ages out, if one will.
    pub fn next_expiry(&self, now: Instant) -> Option<Duration> {
        self.tabs
            .values()
            .flatten()
            .filter(|h| h.busy == 0 && h.live(now))
            .map(|h| LINGER.saturating_sub(now.saturating_duration_since(h.last)))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(key: &str, agent: &str) -> Who {
        Who {
            key: key.into(),
            agent: agent.into(),
            thread: "t".into(),
            seed: key.into(),
            ..Who::default()
        }
    }

    fn tab(s: &str) -> TabId {
        TabId(s.to_string())
    }

    fn began(ticket: u64, w: &Who, t: &TabId) -> DriveEvent {
        DriveEvent::CallBegan {
            ticket,
            who: w.clone(),
            tool: "browser_click".into(),
            title: "Click Sign in".into(),
            detail: None,
            tab: Some(t.clone()),
            behind: false,
        }
    }

    fn ended(ticket: u64, w: &Who, t: &TabId) -> DriveEvent {
        DriveEvent::CallEnded {
            ticket,
            who: w.clone(),
            tool: "browser_click".into(),
            outcome: copper_mcp::drive::Outcome::default(),
            tab: Some(t.clone()),
        }
    }

    #[test]
    fn initials_are_short() {
        assert_eq!(initials("Claude Code"), "CC");
        assert_eq!(initials("phi"), "P");
        assert_eq!(initials("Copper's agent"), "CS");
        assert_eq!(initials(""), "A");
    }

    #[test]
    fn a_call_marks_its_tab_then_lingers_thirty_seconds() {
        let mut h = Hands::default();
        let (a, t) = (who("a", "Claude Code"), tab("t1"));
        let t0 = Instant::now();
        assert_eq!(h.event(&began(1, &a, &t), t0), vec![t.clone()]);
        assert_eq!(h.on(&t, t0).len(), 1);
        assert!(h.ring(&t, t0).is_some());
        h.event(&ended(1, &a, &t), t0);
        assert!(h.ring(&t, t0).is_none(), "the ring is for work in flight");
        assert_eq!(h.on(&t, t0 + Duration::from_secs(29)).len(), 1);
        assert!(h.on(&t, t0 + Duration::from_secs(31)).is_empty());
        assert_eq!(h.next_expiry(t0), Some(LINGER));
    }

    #[test]
    fn a_call_that_ends_elsewhere_leaves_no_mark_where_it_began() {
        let mut h = Hands::default();
        let (a, user, made) = (who("a", "Claude Code"), tab("user"), tab("made"));
        let now = Instant::now();
        h.event(&began(1, &a, &user), now);
        let changed = h.event(&ended(1, &a, &made), now);
        assert!(changed.contains(&user) && changed.contains(&made));
        assert!(h.on(&user, now).is_empty(), "the user's tab stays clean");
        assert_eq!(h.on(&made, now).len(), 1);
        // A hand already on the user's tab stays.
        h.event(&began(2, &a, &user), now);
        h.event(&ended(2, &a, &user), now);
        h.event(&began(3, &a, &user), now);
        h.event(&ended(3, &a, &made), now);
        assert_eq!(h.on(&user, now).len(), 1);
    }

    #[test]
    fn a_new_colour_keeps_clear_of_the_space_tint() {
        let a = who("a", "Claude Code");
        let mut h = Hands::default();
        let now = Instant::now();
        h.event(&began(1, &a, &tab("t")), now);
        let first = h.on(&tab("t"), now)[0].color;
        let mut h = Hands {
            tint: Some(hue(first)),
            ..Default::default()
        };
        h.event(&began(1, &a, &tab("t")), now);
        let second = h.on(&tab("t"), now)[0].color;
        assert_ne!(first, second);
        assert!(!near_hue(hue(second), hue(first)));
        assert!((hue((1.0, 0.0, 0.0))).abs() < 1e-6);
        assert!((hue((0.0, 0.0, 1.0)) - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn two_sessions_wear_two_colours_and_keep_them() {
        let mut h = Hands::default();
        let t = tab("t1");
        let now = Instant::now();
        // Two keys with the same seed would collide: the second moves on.
        let mut a = who("a", "Claude Code");
        let mut b = who("b", "phi");
        a.seed = "same".into();
        b.seed = "same".into();
        h.event(&began(1, &a, &t), now);
        h.event(&began(2, &b, &t), now);
        let hs = h.on(&t, now);
        assert_eq!(hs.len(), 2);
        assert_ne!(hs[0].color, hs[1].color);
        let first = h.colors["a"];
        h.clear();
        h.event(&began(3, &a, &t), now);
        assert_eq!(h.on(&t, now)[0].color, first, "kept for the app's lifetime");
    }

    #[test]
    fn at_most_three_latest_first() {
        let mut h = Hands::default();
        let t = tab("t1");
        let t0 = Instant::now();
        for i in 0..5u64 {
            let w = who(&format!("k{i}"), "Agent");
            h.event(&began(i, &w, &t), t0 + Duration::from_millis(i * 10));
            h.event(&ended(i, &w, &t), t0 + Duration::from_millis(i * 10));
        }
        let hs = h.on(&t, t0 + Duration::from_millis(100));
        assert_eq!(hs.len(), MOST);
        assert_eq!(hs[0].key, "k4");
    }

    #[test]
    fn stop_and_jev_runs() {
        let mut h = Hands::default();
        let (t, j) = (tab("t1"), who("jev", "Jev"));
        let now = Instant::now();
        h.event(
            &DriveEvent::RunBegan {
                who: j.clone(),
                goal: "Find the price".into(),
                tab: t.clone(),
            },
            now,
        );
        let hs = h.on(&t, now);
        assert!(hs[0].jev && hs[0].initials.is_empty() && hs[0].busy == 1);
        assert_eq!(
            h.elsewhere(Some(&tab("other")), now).map(|h| h.key),
            Some("jev".into())
        );
        assert!(h.elsewhere(Some(&t), now).is_none());
        h.event(
            &DriveEvent::RunFinished {
                status: Status::Done,
                note: String::new(),
            },
            now,
        );
        assert_eq!(h.on(&t, now)[0].action, "Done");
        assert_eq!(h.stop("jev", now), vec![t.clone()]);
        assert!(h.on(&t, now)[0].stopped);
        // A Jev run just ends: nothing to let back in.
        assert!(!h.on(&t, now)[0].refused(now));
        assert_eq!(h.let_back_in(), vec![t.clone()]);
        assert!(!h.on(&t, now)[0].stopped);
        h.forget(&t);
        assert!(h.tabs(now).is_empty());
    }
}
