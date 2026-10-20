//! The saved-password list's arithmetic, apart from any window
//! (`Passwords.swift` `shownSites`, `Vault.host(of:)`): Credential
//! Manager's sign-ins grouped by site, the sites used most recently first,
//! narrowed by a search over site and account, and a typed site turned
//! into the host passwords are kept under. Platform-free, so it is tested
//! on any host.

use copper_creds::Login;

/// One site and its accounts.
#[derive(Clone, Debug, PartialEq)]
pub struct Site {
    pub host: String,
    /// Most recently used first, then by name.
    pub logins: Vec<Login>,
}

impl Site {
    /// When any account here was last used.
    fn used(&self) -> f64 {
        self.logins
            .iter()
            .filter_map(|l| l.used)
            .fold(f64::MIN, f64::max)
    }
}

/// `logins` by site — the site used most recently first, then
/// alphabetically — keeping only what `hunt` finds in a site or an account
/// name (a site that matches keeps every account).
pub fn sites(logins: &[Login], hunt: &str) -> Vec<Site> {
    let needle = hunt.trim().to_lowercase();
    let mut out: Vec<Site> = Vec::new();
    for l in logins {
        let site_hit = l.host.to_lowercase().contains(&needle);
        if !needle.is_empty() && !site_hit && !l.user.to_lowercase().contains(&needle) {
            continue;
        }
        match out.iter_mut().find(|s| s.host == l.host) {
            Some(s) => s.logins.push(l.clone()),
            None => out.push(Site {
                host: l.host.clone(),
                logins: vec![l.clone()],
            }),
        }
    }
    for s in &mut out {
        s.logins.sort_by(|a, b| {
            b.used
                .unwrap_or(f64::MIN)
                .total_cmp(&a.used.unwrap_or(f64::MIN))
                .then_with(|| a.user.to_lowercase().cmp(&b.user.to_lowercase()))
        });
    }
    out.sort_by(|a, b| {
        b.used()
            .total_cmp(&a.used())
            .then_with(|| a.host.cmp(&b.host))
    });
    out
}

/// What a typed site is kept under (`Vault.host(of:)`): its host,
/// lowercased, without `www.` — whether typed as an address or a bare
/// name. Empty when it isn't a site.
pub fn host_of(typed: &str) -> String {
    let t = typed.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return String::new();
    }
    let address = if t.contains("://") {
        t.to_string()
    } else {
        format!("https://{t}")
    };
    let Ok(u) = url::Url::parse(&address) else {
        return String::new();
    };
    let host = u.host_str().unwrap_or("").to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    // A name with no dot is a word, not a site — unless it is this PC.
    if host.contains('.') || host == "localhost" {
        host
    } else {
        String::new()
    }
}

/// `1 account` / `N accounts` beside a site with several.
pub fn accounts(n: usize) -> String {
    super::count(n, "account", "accounts")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(host: &str, user: &str, used: Option<f64>) -> Login {
        Login {
            host: host.into(),
            user: user.into(),
            used,
        }
    }

    #[test]
    fn sites_group_newest_first_and_search_narrows() {
        let all = [
            login("a.com", "zed", Some(1.0)),
            login("b.com", "me", Some(5.0)),
            login("a.com", "amy", Some(9.0)),
            login("c.com", "", None),
        ];
        let s = sites(&all, "");
        let hosts: Vec<&str> = s.iter().map(|s| s.host.as_str()).collect();
        assert_eq!(hosts, ["a.com", "b.com", "c.com"]);
        assert_eq!(s[0].logins[0].user, "amy");
        let s = sites(&all, "ME");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].host, "b.com");
        let s = sites(&all, "a.c");
        assert_eq!(s[0].logins.len(), 2);
        assert!(sites(&all, "nothing").is_empty());
    }

    #[test]
    fn typed_sites_become_hosts() {
        assert_eq!(host_of("https://www.Example.com/login"), "example.com");
        assert_eq!(host_of("accounts.google.com"), "accounts.google.com");
        assert_eq!(host_of("localhost:8080"), "localhost");
        assert_eq!(host_of("hello"), "");
        assert_eq!(host_of("not a site"), "");
        assert_eq!(host_of(""), "");
    }

    #[test]
    fn accounts_count() {
        assert_eq!(accounts(1), "1 account");
        assert_eq!(accounts(3), "3 accounts");
    }
}
