//! The preferred-languages list (`Engine::set_preferred_languages`) as the
//! browser process takes it: WebView2's `Language` environment option
//! (the UI language and the first `Accept-Language` entry) and Chromium's
//! `--accept-lang` switch (the whole list, which also becomes
//! `navigator.languages` and the default `Intl` locale).

/// The longest BCP 47 tag accepted (RFC 5646's practical limit).
const MAX_TAG: usize = 35;

/// `languages` cleaned up: trimmed, `_` read as `-` (`en_US`), anything
/// that is not a BCP 47-looking tag dropped, duplicates (case-insensitive)
/// removed, order kept.
pub(crate) fn normalize(languages: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in languages {
        let tag = raw.trim().replace('_', "-");
        if !is_tag(&tag) {
            if !tag.is_empty() {
                tracing::warn!("preferred languages: {raw:?} is not a language tag; skipped");
            }
            continue;
        }
        if !out.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            out.push(tag);
        }
    }
    out
}

/// A BCP 47-looking tag: alphanumeric subtags of 1–8 characters joined by
/// `-`, the first one letters only.
fn is_tag(tag: &str) -> bool {
    if tag.is_empty() || tag.len() > MAX_TAG {
        return false;
    }
    let mut subtags = tag.split('-');
    let first_ok = subtags
        .next()
        .is_some_and(|s| (1..=8).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphabetic()));
    first_ok
        && subtags
            .all(|s| (1..=8).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// The `--accept-lang` switch for a normalized list (`None` when empty: the
/// browser keeps its own choice).
pub(crate) fn accept_lang_switch(languages: &[String]) -> Option<String> {
    (!languages.is_empty()).then(|| format!("--accept-lang={}", languages.join(",")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn cleans_the_list() {
        assert_eq!(
            normalize(&s(&[
                " de-DE ",
                "fr",
                "de-de",
                "en_US",
                "",
                "not a tag",
                "x--y",
                "zh-Hant-TW"
            ])),
            s(&["de-DE", "fr", "en-US", "zh-Hant-TW"])
        );
        assert!(normalize(&s(&["", "  "])).is_empty());
        assert!(normalize(&s(&["1de"])).is_empty());
        assert!(normalize(&s(&["de-DE,fr"])).is_empty());
    }

    #[test]
    fn switch() {
        assert_eq!(
            accept_lang_switch(&s(&["de-DE", "fr"])).as_deref(),
            Some("--accept-lang=de-DE,fr")
        );
        assert_eq!(accept_lang_switch(&[]), None);
    }
}
