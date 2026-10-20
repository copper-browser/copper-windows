//! Settings search (`Fork/SettingsSearch.swift`): the matcher over every
//! page's [`Entry`]s, with no window in sight so it is tested on any host.
//!
//! Matching is per word, after folding case and diacritics. A query word
//! scores against each word of a field: exact, then one-and-many
//! ("shortcut"/"shortcuts"), then prefix, then inside a word, then a typo
//! (one edit, two for words of nine letters or more — a transposition is
//! one; only in words of five letters or more, the first letter kept), then
//! the letters in order. Fields weigh title > keywords > page > section >
//! subtitle, every query word has to land somewhere (synonyms count), and a
//! few whole-query bonuses put the obvious answer first. The weights and
//! thresholds are the Mac's, so the same query ranks the same way.

use std::collections::{BTreeSet, HashMap};

mod words;

use words::{Field, Token, best_in, field, tokens};

/// One thing search can find: a page as a whole, or one of its rows.
///
/// Pages give theirs through `Page::entries` (by default, every titled line
/// of the page's form); the panel adds the page entry itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// The page it is on (`Page::id`).
    pub page: &'static str,
    /// The page's title, for the field search weighs as "page".
    pub page_title: String,
    /// The section it sits in ("" for a page entry).
    pub section: String,
    pub title: String,
    pub subtitle: String,
    /// Other words for it ("dark mode" for Appearance).
    pub keywords: Vec<String>,
    /// What the page scrolls to when the entry is picked: a line's anchor
    /// (`form::Line::anchor`, or its title when it has none) or a page's
    /// own anchor (`Page::anchor`). Empty: the top of the page.
    pub anchor: String,
    /// The whole page, not a row on it.
    pub is_page: bool,
}

/// A ranked match.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub entry: Entry,
    pub score: f64,
    /// Character offsets of the title that matched.
    pub title_marks: BTreeSet<usize>,
    /// Character offsets of the subtitle that matched.
    pub subtitle_marks: BTreeSet<usize>,
}

/// One page's hits, in the order the results draw them.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub page: &'static str,
    pub hits: Vec<Hit>,
}

/// The results of one query: grouped by page (pages in the order of their
/// best hit), and how many rows matched on each page (a page whose only
/// hit is the page itself counts 0, so the rail still lights it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Results {
    pub groups: Vec<Group>,
    pub counts: HashMap<&'static str, usize>,
}

impl Results {
    /// Every hit, in drawn order (what ↑/↓ walk).
    pub fn flat(&self) -> Vec<&Hit> {
        self.groups.iter().flat_map(|g| g.hits.iter()).collect()
    }

    pub fn len(&self) -> usize {
        self.groups.iter().map(|g| g.hits.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Words the no-hits state offers instead (`SettingsResults.suggestions`).
pub const SUGGESTIONS: [&str; 8] = [
    "dark mode",
    "passwords",
    "downloads folder",
    "agents",
    "default browser",
    "updates",
    "block ads",
    "1Password",
];

/// The most hits shown.
const LIMIT: usize = 60;

// MARK: one entry

const W_TITLE: f64 = 1.0;
const W_KEYWORD: f64 = 0.85;
const W_PAGE: f64 = 0.6;
const W_SECTION: f64 = 0.55;
const W_SUBTITLE: f64 = 0.45;

struct Prepared {
    order: usize,
    title: Field,
    subtitle: Field,
    section: Field,
    page: Field,
    keywords: Vec<Field>,
}

fn prepare(order: usize, e: &Entry, page_words: &str) -> Prepared {
    Prepared {
        order,
        title: field(&e.title),
        subtitle: field(&e.subtitle),
        section: field(&e.section),
        page: field(&format!("{} {page_words}", e.page_title)),
        keywords: e.keywords.iter().map(|k| field(k)).collect(),
    }
}

fn score(e: &Entry, p: &Prepared, tokens: &[Token], phrase: &str) -> Option<Hit> {
    let mut total = 0.0;
    let mut title_marks = BTreeSet::new();
    let mut subtitle_marks = BTreeSet::new();
    let mut all_in_title = true;
    let mut all_on_page = true;
    for token in tokens {
        let mut token_best: f64 = 0.0;
        let mut title_best: f64 = 0.0;
        let mut page_best: f64 = 0.0;
        // Lit letters come from the word as typed when it matched; a synonym
        // lights its own only when the typed word didn't.
        let mut title_lit: Option<Vec<usize>> = None;
        let mut subtitle_lit: Option<Vec<usize>> = None;
        let candidates =
            std::iter::once((&token.text, 1.0)).chain(token.alternatives.iter().map(|a| (a, 0.92)));
        for (word, factor) in candidates {
            // A synonym counts only as itself or a word's start, never
            // through a typo of its own.
            let floor = if factor < 1.0 { 0.75 } else { 0.0 };
            let best = |f: &Field| best_in(word, f).filter(|m| m.0 >= floor);
            if let Some((s, lit)) = best(&p.title) {
                token_best = token_best.max(s * W_TITLE * factor);
                title_best = title_best.max(s * factor);
                if s >= 0.45 && title_lit.is_none() {
                    title_lit = Some(lit);
                }
            }
            for k in &p.keywords {
                if let Some((s, _)) = best(k) {
                    token_best = token_best.max(s * W_KEYWORD * factor);
                }
            }
            if let Some((s, _)) = best(&p.page) {
                token_best = token_best.max(s * W_PAGE * factor);
                page_best = page_best.max(s * factor);
            }
            if let Some((s, _)) = best(&p.section) {
                token_best = token_best.max(s * W_SECTION * factor);
            }
            if let Some((s, lit)) = best(&p.subtitle) {
                token_best = token_best.max(s * W_SUBTITLE * factor);
                if s >= 0.55 && subtitle_lit.is_none() {
                    subtitle_lit = Some(lit);
                }
            }
        }
        title_marks.extend(title_lit.unwrap_or_default());
        subtitle_marks.extend(subtitle_lit.unwrap_or_default());
        // Every word has to land somewhere.
        if token_best <= 0.0 {
            return None;
        }
        total += token_best;
        if title_best < 0.5 {
            all_in_title = false;
        }
        if page_best < 0.97 {
            all_on_page = false;
        }
    }
    let mut s = total / tokens.len() as f64 * 100.0;
    let title = &p.title.joined;
    if title == phrase {
        s += 40.0;
    } else if title.starts_with(phrase) {
        s += 22.0 * (phrase.chars().count() as f64 / 4.0).min(1.0);
    } else if title.contains(&format!(" {phrase}")) {
        s += 10.0;
    }
    if p.keywords.iter().any(|k| k.joined == phrase) {
        s += 28.0;
    } else if phrase.chars().count() >= 3 && p.keywords.iter().any(|k| k.joined.starts_with(phrase))
    {
        s += 12.0;
    }
    if all_in_title {
        s += 8.0;
    }
    // The page itself leads when the query names it; its rows get a smaller
    // lift; a page the query doesn't name sinks below rows.
    if e.is_page {
        s += if all_in_title { 12.0 } else { -8.0 };
    } else if all_on_page {
        s += 6.0;
    }
    Some(Hit {
        entry: e.clone(),
        score: s,
        title_marks,
        subtitle_marks,
    })
}

/// The index prepared once (per opening of Settings), searched per keystroke.
pub struct Index {
    entries: Vec<Entry>,
    prepared: Vec<Prepared>,
}

impl Index {
    /// Prepare `entries`; `page_words(page)` are a page's own keywords,
    /// weighed with its title as the "page" field of every entry on it.
    pub fn new(entries: Vec<Entry>, page_words: impl Fn(&str) -> String) -> Index {
        let prepared = entries
            .iter()
            .enumerate()
            .map(|(i, e)| prepare(i, e, &page_words(e.page)))
            .collect();
        Index { entries, prepared }
    }

    /// Ranked hits for `query`, best first, within reach of the best,
    /// grouped by page.
    pub fn search(&self, query: &str) -> Results {
        let tokens = tokens(query);
        if tokens.is_empty() {
            return Results::default();
        }
        let phrase = tokens
            .iter()
            .map(|t| t.text.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(" ");
        let mut hits: Vec<(Hit, usize)> = self
            .entries
            .iter()
            .zip(&self.prepared)
            .filter_map(|(e, p)| score(e, p, &tokens, &phrase).map(|h| (h, p.order)))
            .collect();
        hits.sort_by(|a, b| {
            if (a.0.score - b.0.score).abs() > 0.001 {
                b.0.score.total_cmp(&a.0.score)
            } else {
                a.1.cmp(&b.1)
            }
        });
        let Some(top) = hits.first().map(|h| h.0.score) else {
            return Results::default();
        };
        let floor = (top * 0.42).max(30.0);
        let kept: Vec<Hit> = hits
            .into_iter()
            .map(|h| h.0)
            .filter(|h| h.score >= floor)
            .take(LIMIT)
            .collect();
        group(kept)
    }
}

/// Group ranked hits by page, pages in the order of their best hit.
fn group(hits: Vec<Hit>) -> Results {
    let mut groups: Vec<Group> = Vec::new();
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for h in &hits {
        if !h.entry.is_page {
            *counts.entry(h.entry.page).or_default() += 1;
        }
    }
    for h in &hits {
        counts.entry(h.entry.page).or_insert(0);
    }
    for h in hits {
        match groups.iter_mut().find(|g| g.page == h.entry.page) {
            Some(g) => g.hits.push(h),
            None => groups.push(Group {
                page: h.entry.page,
                hits: vec![h],
            }),
        }
    }
    Results { groups, counts }
}

/// What the results' header says (`resultsLine`).
pub fn results_line(query: &str, results: &Results) -> String {
    let shown = query.trim();
    let n = results.len();
    if n == 0 {
        return "Every page searched".into();
    }
    let pages = results.groups.len();
    let s = if n == 1 { "" } else { "s" };
    let on = if pages > 1 {
        format!(" on {pages} pages")
    } else {
        String::new()
    };
    format!("{n} result{s} for “{shown}”{on}  ·  ↑↓ to choose, Enter to go")
}

#[cfg(test)]
mod tests {
    use super::words::{distance, fold};
    use super::*;

    fn e(page: &'static str, section: &str, title: &str, subtitle: &str, kw: &[&str]) -> Entry {
        Entry {
            page,
            page_title: page[..1].to_uppercase() + &page[1..],
            section: section.into(),
            title: title.into(),
            subtitle: subtitle.into(),
            keywords: kw.iter().map(|s| s.to_string()).collect(),
            anchor: title.into(),
            is_page: false,
        }
    }

    fn index() -> Index {
        let mut v = vec![
            Entry {
                page: "passwords",
                page_title: "Passwords".into(),
                title: "Passwords".into(),
                subtitle: "Saved sign-ins, password managers".into(),
                keywords: vec!["logins".into(), "keychain".into()],
                is_page: true,
                ..Entry::default()
            },
            e(
                "general",
                "Copper on this PC",
                "Appearance",
                "Light, dark, or whatever Windows is doing",
                &["dark mode", "light mode", "theme"],
            ),
            e(
                "passwords",
                "Saved passwords",
                "Offer to save passwords",
                "Asked once per site",
                &["save passwords"],
            ),
            e(
                "downloads",
                "Downloads",
                "Save to",
                "The folder downloads go to",
                &["downloads folder", "download location"],
            ),
            e(
                "about",
                "Keyboard shortcuts",
                "Keyboard shortcuts",
                "Every keystroke Copper answers to",
                &["hotkeys"],
            ),
        ];
        v.push(e(
            "privacy",
            "Blocking",
            "Block ads and trackers",
            "Third parties whose only job is to watch",
            &["adblock", "ad blocker"],
        ));
        Index::new(v, |_| String::new())
    }

    #[test]
    fn folds_case_and_accents() {
        assert_eq!(fold("Ça Été"), "ca ete");
        assert_eq!(fold("ＡＢＣ"), "abc");
    }

    #[test]
    fn distance_counts_a_transposition_once() {
        let a: Vec<char> = "privcay".chars().collect();
        let b: Vec<char> = "privacy".chars().collect();
        assert_eq!(distance(&a, &b, 2), 1);
        let c: Vec<char> = "xyz".chars().collect();
        assert_eq!(distance(&a, &c, 1), 2);
    }

    #[test]
    fn dark_mode_finds_appearance_first() {
        let r = index().search("dark mode");
        let flat = r.flat();
        assert_eq!(flat[0].entry.title, "Appearance");
    }

    #[test]
    fn a_typo_still_finds_its_row() {
        let r = index().search("dowloads");
        assert!(r.flat().iter().any(|h| h.entry.title == "Save to"));
    }

    #[test]
    fn plurals_match() {
        let r = index().search("shortcut");
        assert_eq!(r.flat()[0].entry.title, "Keyboard shortcuts");
    }

    #[test]
    fn synonyms_reach_other_words() {
        let r = index().search("ublock");
        assert_eq!(r.flat()[0].entry.title, "Block ads and trackers");
    }

    #[test]
    fn the_page_leads_when_named() {
        let r = index().search("passwords");
        assert_eq!(r.groups[0].page, "passwords");
        assert!(r.groups[0].hits[0].entry.is_page);
        assert_eq!(r.counts.get("passwords"), Some(&1));
    }

    #[test]
    fn every_word_has_to_land() {
        assert!(index().search("appearance zebra").is_empty());
        assert!(index().search("").is_empty());
        assert!(index().search("qqqqq").is_empty());
    }

    #[test]
    fn marks_light_the_matched_letters() {
        let r = index().search("appear");
        let h = r.flat()[0];
        assert_eq!(h.entry.title, "Appearance");
        assert_eq!(
            h.title_marks.iter().copied().collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn the_results_line_counts() {
        let idx = index();
        let r = idx.search("passwords");
        assert!(results_line("passwords", &r).starts_with("2 results for “passwords”"));
        assert_eq!(
            results_line("x", &Results::default()),
            "Every page searched"
        );
    }
}
