//! The search's words (`SettingsSearch.swift` › prepared fields and
//! matching): folding case and accents, splitting a field into words and
//! run-together pairs, a query's words with their synonyms, and how well
//! one query word matches one field — exact, plural, prefix, inside, a
//! typo, or its letters in order.

#[derive(Clone, Debug)]
pub(super) struct Word {
    pub(super) chars: Vec<char>,
    /// Where the word sits in the field, in characters.
    pub(super) start: usize,
    pub(super) span: usize,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Field {
    pub(super) words: Vec<Word>,
    /// Neighbouring words run together ("auto-lock" → "autolock"), so a
    /// query typed as one word still finds two.
    pub(super) pairs: Vec<Word>,
    pub(super) joined: String,
}

/// Lowercase without accents: what both sides are compared as.
pub(super) fn fold(text: &str) -> String {
    text.chars().flat_map(fold_char).collect()
}

/// One character folded: lowercase, accents and width dropped.
pub(super) fn fold_char(c: char) -> Vec<char> {
    let base = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' => {
            'a'
        }
        'ç' | 'Ç' | 'ć' | 'č' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'È' | 'É' | 'Ê' | 'Ë' | 'Ē' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
        'ñ' | 'Ñ' | 'ń' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
        'ý' | 'ÿ' | 'Ý' => 'y',
        'ś' | 'š' | 'Ś' | 'Š' => 's',
        'ź' | 'ž' | 'ż' | 'Ź' | 'Ž' | 'Ż' => 'z',
        // Fullwidth ASCII.
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    };
    base.to_lowercase().collect()
}

pub(super) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

pub(super) fn field(text: &str) -> Field {
    let chars: Vec<char> = text.chars().collect();
    let mut words = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_word_char(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && is_word_char(chars[i]) {
            i += 1;
        }
        let raw: String = chars[start..i].iter().collect();
        let mut folded: Vec<char> = fold(&raw).chars().collect();
        // A fold that changed the length would light the wrong letters.
        if folded.len() != i - start {
            folded = raw.to_lowercase().chars().collect();
        }
        words.push(Word {
            chars: folded,
            start,
            span: i - start,
        });
    }
    let pairs = words
        .windows(2)
        .map(|w| Word {
            chars: w[0].chars.iter().chain(&w[1].chars).copied().collect(),
            start: w[0].start,
            span: w[1].start + w[1].span - w[0].start,
        })
        .collect();
    let joined = words
        .iter()
        .map(|w| w.chars.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ");
    Field {
        words,
        pairs,
        joined,
    }
}

// MARK: the query

const STOP_WORDS: [&str; 17] = [
    "the", "a", "an", "to", "of", "in", "on", "for", "my", "and", "with", "how", "do", "i", "is",
    "where", "can",
];

/// Other words for a query word. The entries' keywords carry most of the
/// vocabulary; these are the ones that cut across pages.
pub(super) fn synonyms(word: &str) -> &'static [&'static str] {
    match word {
        "pw" => &["password", "passwords"],
        "pwd" | "passwd" => &["password"],
        "login" => &["sign", "password"],
        "logins" => &["sign", "passwords"],
        "signin" => &["sign"],
        "keychain" => &["passwords"],
        "credential" => &["password"],
        "credentials" => &["passwords"],
        "ai" | "llm" => &["intelligence", "model"],
        "gpt" => &["model", "openai"],
        "claude" => &["claude", "intelligence"],
        "model" => &["model", "intelligence"],
        "anthropic" => &["claude"],
        "proxy" => &["agents", "mcp"],
        "mcp" => &["agents"],
        "bot" => &["agents", "bots"],
        "bots" => &["agents"],
        "automation" | "automate" => &["agents", "script"],
        "update" => &["updates"],
        "upgrade" => &["updates", "update"],
        "adblock" | "adblocker" | "ublock" => &["block", "ads"],
        "tracker" => &["trackers", "block"],
        "tracking" => &["trackers"],
        "dark" | "darkmode" => &["appearance"],
        "night" => &["appearance", "dark"],
        "theme" => &["appearance", "look"],
        "color" => &["colour", "colours"],
        "colors" => &["colours"],
        "colour" => &["color", "colours"],
        "download" => &["downloads"],
        "folder" => &["save", "directory"],
        "shortcut" | "hotkey" | "hotkeys" | "keybinding" | "keybindings" => &["shortcuts"],
        "extension" | "addon" | "plugin" | "plugins" => &["extensions"],
        "icloud" => &["cloud", "sync"],
        "memory" => &["sleep"],
        "spell" => &["spelling"],
        "1pw" | "onepassword" => &["1password"],
        "mic" => &["microphone"],
        "webcam" => &["camera"],
        "workspace" | "workspaces" => &["spaces"],
        "vertical" => &["sidebar"],
        "browser" => &["browser", "default"],
        "dictate" => &["dictation", "voice"],
        "dictation" | "speech" => &["voice"],
        "transcribe" => &["transcription", "voice"],
        "stt" => &["speech", "voice"],
        "ptt" => &["talk"],
        // Windows' own nouns for the Mac's.
        "credentialmanager" => &["passwords"],
        "hello" => &["passwords"],
        _ => &[],
    }
}

pub(super) struct Token {
    pub(super) text: Vec<char>,
    pub(super) alternatives: Vec<Vec<char>>,
}

pub(super) fn tokens(query: &str) -> Vec<Token> {
    let words: Vec<String> = field(query)
        .words
        .into_iter()
        .map(|w| w.chars.into_iter().collect())
        .collect();
    let kept: Vec<&String> = words
        .iter()
        .filter(|w| !STOP_WORDS.contains(&w.as_str()))
        .collect();
    let use_words: Vec<&String> = if kept.is_empty() {
        words.iter().collect()
    } else {
        kept
    };
    use_words
        .into_iter()
        .map(|w| Token {
            text: w.chars().collect(),
            alternatives: synonyms(w)
                .iter()
                .filter(|s| **s != w.as_str())
                .map(|s| s.chars().collect())
                .collect(),
        })
        .collect()
}

// MARK: one word against one word

/// How well query word `q` matches field word `w`, and which of `w`'s
/// letters it lit.
pub(super) fn word_match(q: &[char], w: &[char]) -> Option<(f64, Vec<usize>)> {
    if q.is_empty() || w.is_empty() {
        return None;
    }
    if q == w {
        return Some((1.0, (0..w.len()).collect()));
    }
    // One and many are the same thing: "shortcut" / "shortcuts".
    if q.len() >= 3 && (plural(q, w) || plural(w, q)) {
        return Some((0.97, (0..q.len().min(w.len())).collect()));
    }
    if w.len() > q.len() && w.starts_with(q) {
        // Two letters are weak evidence of a word; four are strong.
        let start = match q.len() {
            n if n >= 4 => 0.80,
            3 => 0.75,
            _ => 0.70,
        };
        return Some((
            start + 0.18 * q.len() as f64 / w.len() as f64,
            (0..q.len()).collect(),
        ));
    }
    if q.len() >= 3
        && let Some(at) = find(q, w)
    {
        return Some((0.55, (at..at + q.len()).collect()));
    }
    // A slip keeps the first letter, and only in words of five or more.
    if q.len() >= 5 && q[0] == w[0] {
        let most = if q.len() >= 9 { 2 } else { 1 };
        let whole = distance(q, w, most);
        if whole <= most && q.len().abs_diff(w.len()) <= most {
            return Some((if whole == 1 { 0.62 } else { 0.48 }, (0..w.len()).collect()));
        }
        // Still typing, with a slip: the word's start, give or take a letter.
        if w.len() > q.len() {
            let best = [q.len() - 1, q.len(), q.len() + 1]
                .into_iter()
                .filter(|l| *l > 2 && *l <= w.len())
                .map(|l| distance(q, &w[..l], 1))
                .min()
                .unwrap_or(usize::MAX);
            if best <= 1 {
                return Some((0.5, (0..q.len().min(w.len())).collect()));
            }
        }
    }
    // The letters in order ("pswd"); one letter short of the word is a
    // slip, not shorthand, so it takes the five letters a slip does.
    if q.len() >= 3
        && q[0] == w[0]
        && (w.len().saturating_sub(q.len()) > 1 || q.len() >= 5)
        && let Some(lit) = subsequence(q, w)
    {
        let spread = (lit[lit.len() - 1] - lit[0] + 1) as f64;
        return Some((0.35 + 0.25 * q.len() as f64 / spread, lit));
    }
    None
}

/// `many` is `one` with an s.
pub(super) fn plural(many: &[char], one: &[char]) -> bool {
    many.len() == one.len() + 1 && many.last() == Some(&'s') && &many[..one.len()] == one
}

pub(super) fn find(q: &[char], w: &[char]) -> Option<usize> {
    if q.len() > w.len() {
        return None;
    }
    (0..=w.len() - q.len()).find(|&s| &w[s..s + q.len()] == q)
}

pub(super) fn subsequence(q: &[char], w: &[char]) -> Option<Vec<usize>> {
    let mut lit = Vec::with_capacity(q.len());
    let mut j = 0;
    for (i, c) in w.iter().enumerate() {
        if j < q.len() && *c == q[j] {
            lit.push(i);
            j += 1;
        }
    }
    (j == q.len()).then_some(lit)
}

/// Optimal string alignment distance (Levenshtein with adjacent
/// transpositions), giving up past `cap`.
pub(super) fn distance(a: &[char], b: &[char], cap: usize) -> usize {
    if a.len().abs_diff(b.len()) > cap {
        return cap + 1;
    }
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut before = vec![0usize; b.len() + 1];
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        current[0] = i;
        let mut row_best = current[0];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (previous[j] + 1)
                .min(current[j - 1] + 1)
                .min(previous[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(before[j - 2] + 1);
            }
            current[j] = v;
            row_best = row_best.min(v);
        }
        if row_best > cap {
            return cap + 1;
        }
        std::mem::swap(&mut before, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// The best match of `q` anywhere in `f`: its score and the field's
/// character offsets it lit.
pub(super) fn best_in(q: &[char], f: &Field) -> Option<(f64, Vec<usize>)> {
    let mut top: Option<(f64, Vec<usize>)> = None;
    for w in &f.words {
        if let Some((s, lit)) = word_match(q, &w.chars)
            && s > top.as_ref().map_or(0.0, |t| t.0)
        {
            let last = w.start + w.span - 1;
            top = Some((s, lit.iter().map(|i| (w.start + i).min(last)).collect()));
        }
    }
    if q.len() >= 4 {
        for p in &f.pairs {
            // Two words run together ("darkmode"), whole, from the start or
            // with one slip; never letters picked out of the pair.
            if let Some((s, _)) = word_match(q, &p.chars)
                && s >= 0.62
                && s > top.as_ref().map_or(0.0, |t| t.0)
            {
                top = Some((s * 0.95, (p.start..p.start + p.span).collect()));
            }
        }
    }
    top
}
