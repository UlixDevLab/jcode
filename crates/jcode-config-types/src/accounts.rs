//! Per-directory account (subscription) binding configuration.
//!
//! Lets a session pick which logged-in provider account to bill by looking at
//! the session's working directory. The motivating case is one machine holding
//! a company subscription and a personal subscription, where work under a
//! company directory must never bill the personal account and vice versa.
//!
//! Example `~/.jcode/config.toml`:
//!
//! ```toml
//! [accounts.openai]
//! default = "openai-2"
//!
//! [[accounts.openai.rules]]
//! path = "~/Documents/LeGrin.tech/ULIX"
//! account = "openai-1"
//! ```
//!
//! Matching is by path prefix on canonical directory boundaries, and the
//! **most specific** (longest) matching prefix wins, so a nested exception can
//! override a broader rule.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Account bindings for every provider, keyed by provider prefix
/// (`"openai"`, `"claude"`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(transparent)]
pub struct AccountsConfig {
    pub providers: BTreeMap<String, ProviderAccountsConfig>,
}

impl AccountsConfig {
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn provider(&self, prefix: &str) -> Option<&ProviderAccountsConfig> {
        self.providers.get(prefix)
    }
}

/// Account bindings for one provider.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderAccountsConfig {
    /// Account label used when no rule matches. Unset falls back to the
    /// provider's persisted active account, preserving today's behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,

    /// Path rules, evaluated most-specific-first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<AccountRule>,
}

/// One directory-to-account binding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountRule {
    /// Directory prefix. `~` is expanded. The rule matches the directory
    /// itself and everything beneath it. A trailing `/**` or `/*` is accepted
    /// and ignored so glob-looking config reads naturally.
    pub path: String,

    /// Account label to bind, e.g. `openai-1`.
    pub account: String,
}

/// Strip a trailing glob suffix so `"~/x/**"` and `"~/x"` behave identically.
pub fn normalize_rule_path(path: &str) -> &str {
    let trimmed = path.trim().trim_end_matches('/');
    for suffix in ["/**", "/*"] {
        if let Some(stripped) = trimmed.strip_suffix(suffix) {
            return stripped.trim_end_matches('/');
        }
    }
    if trimmed == "**" || trimmed == "*" {
        return "";
    }
    trimmed
}

/// True when `dir` is `prefix` or lies beneath it.
///
/// Compares on `/`-delimited segment boundaries so `/a/bcd` does not match the
/// prefix `/a/bc`.
pub fn path_is_within(dir: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    let dir = dir.trim_end_matches('/');
    let prefix = prefix.trim_end_matches('/');
    if dir == prefix {
        return true;
    }
    dir.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Outcome of resolving a directory to an account label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountResolution {
    /// A rule matched. Carries the label and the rule path that matched.
    Rule { account: String, path: String },
    /// No rule matched; the configured provider default applies.
    Default(String),
    /// Nothing configured; the caller should keep its existing behavior.
    Unconfigured,
}

impl AccountResolution {
    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Rule { account, .. } => Some(account),
            Self::Default(account) => Some(account),
            Self::Unconfigured => None,
        }
    }
}

/// Resolve `working_dir` to an account label for one provider.
///
/// `expand` turns a configured path into an absolute one (`~` expansion);
/// keeping it as a callback lets this crate stay free of home-directory logic.
/// `working_dir` is expected to already be absolute.
///
/// The most specific matching rule wins. Ties are broken by config order.
pub fn resolve_account_for_dir<F>(
    config: &ProviderAccountsConfig,
    working_dir: &str,
    expand: F,
) -> AccountResolution
where
    F: Fn(&str) -> String,
{
    let dir = working_dir.trim_end_matches('/');

    let mut best: Option<(usize, &AccountRule, String)> = None;
    for rule in &config.rules {
        let expanded = expand(normalize_rule_path(&rule.path));
        let expanded = expanded.trim_end_matches('/').to_string();
        if !path_is_within(dir, &expanded) {
            continue;
        }
        let specificity = expanded.len();
        let is_better = best
            .as_ref()
            .is_none_or(|(best_len, _, _)| specificity > *best_len);
        if is_better {
            best = Some((specificity, rule, expanded));
        }
    }

    if let Some((_, rule, expanded)) = best {
        return AccountResolution::Rule {
            account: rule.account.clone(),
            path: expanded,
        };
    }

    match &config.default {
        Some(default) => AccountResolution::Default(default.clone()),
        None => AccountResolution::Unconfigured,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(path: &str) -> String {
        path.replace('~', "/home/u")
    }

    fn rule(path: &str, account: &str) -> AccountRule {
        AccountRule {
            path: path.to_string(),
            account: account.to_string(),
        }
    }

    fn config(default: Option<&str>, rules: Vec<AccountRule>) -> ProviderAccountsConfig {
        ProviderAccountsConfig {
            default: default.map(str::to_string),
            rules,
        }
    }

    #[test]
    fn unconfigured_provider_resolves_to_nothing() {
        let cfg = config(None, vec![]);
        assert_eq!(
            resolve_account_for_dir(&cfg, "/home/u/x", expand),
            AccountResolution::Unconfigured
        );
    }

    #[test]
    fn falls_back_to_default_when_no_rule_matches() {
        let cfg = config(Some("openai-2"), vec![rule("~/work/ULIX", "openai-1")]);
        assert_eq!(
            resolve_account_for_dir(&cfg, "/home/u/personal", expand).label(),
            Some("openai-2")
        );
    }

    #[test]
    fn matches_the_rule_directory_itself_and_descendants() {
        let cfg = config(Some("openai-2"), vec![rule("~/work/ULIX", "openai-1")]);
        for dir in [
            "/home/u/work/ULIX",
            "/home/u/work/ULIX/",
            "/home/u/work/ULIX/repo/src",
        ] {
            assert_eq!(
                resolve_account_for_dir(&cfg, dir, expand).label(),
                Some("openai-1"),
                "dir {dir}"
            );
        }
    }

    #[test]
    fn does_not_match_sibling_with_shared_name_prefix() {
        // The whole point of segment-boundary matching: ULIX-archive is a
        // different project and must not bill the company subscription.
        let cfg = config(Some("openai-2"), vec![rule("~/work/ULIX", "openai-1")]);
        assert_eq!(
            resolve_account_for_dir(&cfg, "/home/u/work/ULIX-archive", expand).label(),
            Some("openai-2")
        );
    }

    #[test]
    fn most_specific_rule_wins_regardless_of_order() {
        let cfg = config(
            Some("openai-2"),
            vec![
                rule("~/work/ULIX/vendor", "openai-2"),
                rule("~/work/ULIX", "openai-1"),
            ],
        );
        assert_eq!(
            resolve_account_for_dir(
                &cfg,
                "~/work/ULIX/vendor/lib".replace('~', "/home/u").as_str(),
                expand
            )
            .label(),
            Some("openai-2")
        );
        assert_eq!(
            resolve_account_for_dir(&cfg, "/home/u/work/ULIX/src", expand).label(),
            Some("openai-1")
        );
    }

    #[test]
    fn glob_suffixes_are_accepted_and_ignored() {
        assert_eq!(normalize_rule_path("~/work/ULIX/**"), "~/work/ULIX");
        assert_eq!(normalize_rule_path("~/work/ULIX/*"), "~/work/ULIX");
        assert_eq!(normalize_rule_path("~/work/ULIX/"), "~/work/ULIX");

        let cfg = config(Some("openai-2"), vec![rule("~/work/ULIX/**", "openai-1")]);
        assert_eq!(
            resolve_account_for_dir(&cfg, "/home/u/work/ULIX/src", expand).label(),
            Some("openai-1")
        );
        // A `/**` rule still covers the root directory itself, which is what a
        // user means by "this project uses the company account".
        assert_eq!(
            resolve_account_for_dir(&cfg, "/home/u/work/ULIX", expand).label(),
            Some("openai-1")
        );
    }

    #[test]
    fn rule_reports_the_matched_path_for_logging() {
        let cfg = config(Some("openai-2"), vec![rule("~/work/ULIX", "openai-1")]);
        match resolve_account_for_dir(&cfg, "/home/u/work/ULIX/src", expand) {
            AccountResolution::Rule { account, path } => {
                assert_eq!(account, "openai-1");
                assert_eq!(path, "/home/u/work/ULIX");
            }
            other => panic!("expected a rule match, got {other:?}"),
        }
    }

    #[test]
    fn catch_all_rule_matches_everything() {
        let cfg = config(None, vec![rule("**", "openai-1")]);
        assert_eq!(
            resolve_account_for_dir(&cfg, "/anywhere/at/all", expand).label(),
            Some("openai-1")
        );
    }

    #[test]
    fn config_round_trips_through_toml() {
        let toml = r#"
[openai]
default = "openai-2"

[[openai.rules]]
path = "~/Documents/LeGrin.tech/ULIX"
account = "openai-1"
"#;
        let parsed: AccountsConfig = toml::from_str(toml).expect("parse");
        let openai = parsed.provider("openai").expect("openai section");
        assert_eq!(openai.default.as_deref(), Some("openai-2"));
        assert_eq!(openai.rules.len(), 1);
        assert_eq!(openai.rules[0].account, "openai-1");

        let rendered = toml::to_string(&parsed).expect("serialize");
        let reparsed: AccountsConfig = toml::from_str(&rendered).expect("reparse");
        assert_eq!(reparsed, parsed);
    }
}
