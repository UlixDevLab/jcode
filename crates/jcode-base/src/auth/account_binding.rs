//! Resolve which provider account a session should use, from its working
//! directory plus `[accounts.<provider>]` in the global config.
//!
//! This is the policy layer. It answers "which label", never "which token".
//! Credential loading stays in `auth::codex` / `auth::claude`.
//!
//! ## Why rules may name an email or account id
//!
//! Account labels are **positional**: [`super::account_store::relabel_accounts`]
//! rewrites every label to `<prefix>-<index>` on load, so `openai-2` means "the
//! second account in the file", not a stable identity. If accounts are removed
//! or reordered, `openai-2` silently designates a different subscription, which
//! for this feature means billing the wrong account.
//!
//! So a rule's `account` field is matched against the label, the email, **and**
//! the provider account id. Configuring an email is strongly preferred because
//! it is stable and human-checkable.

use jcode_config_types::{AccountResolution, ProviderAccountsConfig, resolve_account_for_dir};

/// A logged-in account, reduced to the identities a config rule may name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIdentity {
    /// Positional label, e.g. `openai-1`.
    pub label: String,
    /// Account email, when known.
    pub email: Option<String>,
    /// Provider-side account id, when known.
    pub account_id: Option<String>,
}

impl AccountIdentity {
    pub fn new(
        label: impl Into<String>,
        email: Option<String>,
        account_id: Option<String>,
    ) -> Self {
        Self {
            label: label.into(),
            email,
            account_id,
        }
    }

    /// True when `needle` names this account by label, email, or account id.
    /// Email and label comparisons are case-insensitive.
    fn matches(&self, needle: &str) -> bool {
        let needle = needle.trim();
        if self.label.eq_ignore_ascii_case(needle) {
            return true;
        }
        if self
            .email
            .as_deref()
            .is_some_and(|email| email.eq_ignore_ascii_case(needle))
        {
            return true;
        }
        self.account_id.as_deref() == Some(needle)
    }
}

/// Expand a leading `~` using the real user home directory.
///
/// Deliberately uses the OS home rather than `JCODE_HOME`: these rules describe
/// where the user's *projects* live, which is unaffected by relocating jcode's
/// own state directory.
fn expand_home(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else {
        return path.to_string();
    };
    let Some(home) = dirs::home_dir() else {
        return path.to_string();
    };
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() {
        return home.to_string_lossy().into_owned();
    }
    home.join(rest).to_string_lossy().into_owned()
}

/// Resolve the account label bound to `working_dir` for `provider_prefix`
/// (`"openai"`, `"claude"`).
///
/// Returns `None` when nothing is configured or the configured account is not
/// logged in, meaning the caller keeps its existing behavior (runtime override,
/// then persisted active account).
pub fn resolve_label_for_dir(
    provider_prefix: &str,
    working_dir: &str,
    accounts: &[AccountIdentity],
) -> Option<String> {
    let config = crate::config::config();
    let provider_config = config.accounts.provider(provider_prefix)?;
    resolve_label_with_config(provider_config, provider_prefix, working_dir, accounts)
}

/// Testable core of [`resolve_label_for_dir`].
pub fn resolve_label_with_config(
    provider_config: &ProviderAccountsConfig,
    provider_prefix: &str,
    working_dir: &str,
    accounts: &[AccountIdentity],
) -> Option<String> {
    let log_key = format!("account-binding|{provider_prefix}|{working_dir}");
    let lookup = |needle: &str| {
        accounts
            .iter()
            .find(|account| account.matches(needle))
            .map(|account| account.label.clone())
    };

    match resolve_account_for_dir(provider_config, working_dir, expand_home) {
        AccountResolution::Unconfigured => None,
        AccountResolution::Rule { account, path } => {
            if let Some(label) = lookup(&account) {
                crate::logging::info_if_changed(
                    &log_key,
                    format!(
                        "Account binding: {working_dir} matched rule {path} -> {provider_prefix} account '{account}' ({label})"
                    ),
                );
                return Some(label);
            }
            crate::logging::warn_if_changed(
                &log_key,
                format!(
                    "Account binding rule for {path} names {provider_prefix} account '{account}', which is not logged in. \
                 Falling back to the configured default. Run `jcode login --provider {provider_prefix}` or fix [accounts.{provider_prefix}] in config.toml."
                ),
            );
            let default = provider_config.default.as_deref()?;
            lookup(default).or_else(|| {
                crate::logging::warn_if_changed(&log_key, format!(
                    "Account binding default for {provider_prefix} names '{default}', which is not logged in either."
                ));
                None
            })
        }
        AccountResolution::Default(account) => match lookup(&account) {
            Some(label) => {
                crate::logging::info_if_changed(
                    &log_key,
                    format!(
                        "Account binding: {working_dir} matched no rule -> {provider_prefix} default account '{account}' ({label})"
                    ),
                );
                Some(label)
            }
            None => {
                crate::logging::warn_if_changed(
                    &log_key,
                    format!(
                        "Account binding default for {provider_prefix} names account '{account}', which is not logged in. \
                     Using the persisted active account instead."
                    ),
                );
                None
            }
        },
    }
}

/// Accounts that an explicit path rule binds to a directory, for
/// `provider_prefix`.
///
/// These are "reserved": a rule exists precisely because that account's spend
/// is meant to be scoped to certain directories, so automatic cross-account
/// failover must neither move *into* one (billing a company subscription for
/// unrelated work) nor move *away* from one (billing personal quota for
/// company work). Callers use this to filter failover candidates.
///
/// Returns labels, resolved through [`AccountIdentity::matches`] so a rule may
/// name an email or account id. Accounts named only as a `default` are not
/// reserved: the default is the intended catch-all.
pub fn reserved_labels(provider_prefix: &str, accounts: &[AccountIdentity]) -> Vec<String> {
    let config = crate::config::config();
    let Some(provider_config) = config.accounts.provider(provider_prefix) else {
        return Vec::new();
    };
    reserved_labels_with_config(provider_config, accounts)
}

/// Testable core of [`reserved_labels`].
pub fn reserved_labels_with_config(
    provider_config: &ProviderAccountsConfig,
    accounts: &[AccountIdentity],
) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for rule in &provider_config.rules {
        let Some(account) = accounts
            .iter()
            .find(|account| account.matches(&rule.account))
        else {
            continue;
        };
        if !labels.contains(&account.label) {
            labels.push(account.label.clone());
        }
    }
    labels
}

/// Whether automatic failover may switch between `from` and `to`.
///
/// Denied when either side is reserved by a path rule. The exhaustion is then
/// surfaced to the user instead of silently crossing a billing boundary.
pub fn failover_allowed(provider_prefix: &str, from: Option<&str>, to: &str) -> bool {
    let accounts = match provider_prefix {
        "openai" => crate::auth::codex::account_identities(),
        _ => return true,
    };
    let reserved = reserved_labels(provider_prefix, &accounts);
    if reserved.is_empty() {
        return true;
    }
    let reserved_contains = |label: &str| reserved.iter().any(|r| r == label);
    if reserved_contains(to) {
        crate::logging::info(&format!(
            "Account failover to '{to}' blocked: it is bound to a directory by an \
             [accounts.{provider_prefix}] rule. Not billing it for unrelated work."
        ));
        return false;
    }
    if let Some(from) = from
        && reserved_contains(from)
    {
        crate::logging::info(&format!(
            "Account failover away from '{from}' blocked: it is bound to a directory by an \
             [accounts.{provider_prefix}] rule. Surfacing exhaustion instead of billing '{to}'."
        ));
        return false;
    }
    true
}

/// Short, human-meaningful name for the account bound to `working_dir`, for
/// display in the TUI.
///
/// Prefers the email local part, because the positional label (`openai-1`)
/// tells the user nothing about *which subscription is being billed*, which is
/// the entire question this feature exists to answer.
///
/// Returns `None` when no binding is configured, so UI that has nothing useful
/// to say stays quiet rather than showing a meaningless label.
pub fn bound_account_display(provider_prefix: &str, working_dir: &str) -> Option<String> {
    let accounts = match provider_prefix {
        "openai" => crate::auth::codex::account_identities(),
        _ => return None,
    };
    if accounts.len() < 2 {
        // One account cannot be confused with another, so a badge would be
        // pure noise.
        return None;
    }
    let label = resolve_label_for_dir(provider_prefix, working_dir, &accounts)?;
    let identity = accounts.iter().find(|a| a.label == label)?;
    Some(match identity.email.as_deref() {
        Some(email) => email.split('@').next().unwrap_or(email).to_string(),
        None => identity.label.clone(),
    })
}

/// Account label pinned for the provider instance currently being constructed,
/// keyed by provider prefix.
///
/// Provider runtimes are built by registry factories that take no arguments and
/// load credentials from the process-global active account. Rather than thread
/// a label through every factory signature, session creation pins the resolved
/// label for the duration of the (synchronous) construction and the OpenAI
/// factory consults it.
///
/// Thread-local rather than a global: two sessions may be created concurrently
/// on different threads and must not observe each other's pin. It is
/// deliberately *not* a `tokio::task_local`, which would not survive the
/// `spawn` boundaries used by refresh work.
mod construction_pin {
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static PINNED: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    }

    /// Restores the previous pin on drop so nested or sequential constructions
    /// cannot leak a label into unrelated provider builds.
    pub struct PinGuard {
        prefix: String,
        previous: Option<String>,
    }

    impl Drop for PinGuard {
        fn drop(&mut self) {
            PINNED.with(|pinned| {
                let mut pinned = pinned.borrow_mut();
                match self.previous.take() {
                    Some(previous) => pinned.insert(self.prefix.clone(), previous),
                    None => pinned.remove(&self.prefix),
                }
            });
        }
    }

    pub fn pin(prefix: &str, label: String) -> PinGuard {
        let previous = PINNED.with(|pinned| pinned.borrow_mut().insert(prefix.to_string(), label));
        PinGuard {
            prefix: prefix.to_string(),
            previous,
        }
    }

    pub fn current(prefix: &str) -> Option<String> {
        PINNED.with(|pinned| pinned.borrow().get(prefix).cloned())
    }
}

pub use construction_pin::PinGuard;

/// Pin `label` as the account for `provider_prefix` while the returned guard
/// lives. Used around synchronous provider construction at session creation.
pub fn pin_account_for_construction(provider_prefix: &str, label: String) -> PinGuard {
    construction_pin::pin(provider_prefix, label)
}

/// The account label pinned for the provider currently being constructed.
pub fn pinned_account_for_construction(provider_prefix: &str) -> Option<String> {
    construction_pin::current(provider_prefix)
}

/// Resolve and pin the account for a new session rooted at `working_dir`.
///
/// Returns `None` (and pins nothing) when no binding applies, leaving existing
/// global behavior intact.
pub fn pin_for_new_session(provider_prefix: &str, working_dir: &str) -> Option<PinGuard> {
    let accounts = match provider_prefix {
        "openai" => crate::auth::codex::account_identities(),
        _ => return None,
    };
    if accounts.len() < 2 {
        // With zero or one account there is nothing to route between, so skip
        // the work and keep logs quiet.
        return None;
    }
    let label = resolve_label_for_dir(provider_prefix, working_dir, &accounts)?;
    Some(pin_account_for_construction(provider_prefix, label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_config_types::AccountRule;

    fn accounts() -> Vec<AccountIdentity> {
        vec![
            AccountIdentity::new(
                "openai-1",
                Some("it@ulixtravel.com".to_string()),
                Some("7eb515b8".to_string()),
            ),
            AccountIdentity::new(
                "openai-2",
                Some("daniil.kovaliov@gmail.com".to_string()),
                Some("b8ff4eec".to_string()),
            ),
        ]
    }

    fn config_with(default: Option<&str>, rules: Vec<(&str, &str)>) -> ProviderAccountsConfig {
        ProviderAccountsConfig {
            default: default.map(str::to_string),
            rules: rules
                .into_iter()
                .map(|(path, account)| AccountRule {
                    path: path.to_string(),
                    account: account.to_string(),
                })
                .collect(),
        }
    }

    fn resolve(cfg: &ProviderAccountsConfig, dir: &str) -> Option<String> {
        resolve_label_with_config(cfg, "openai", dir, &accounts())
    }
    #[test]
    fn returns_none_when_provider_has_no_config() {
        assert_eq!(resolve(&config_with(None, vec![]), "/tmp/x"), None);
    }
    #[test]
    fn matched_rule_wins_when_the_account_exists() {
        let cfg = config_with(Some("openai-2"), vec![("/work/ULIX", "openai-1")]);
        assert_eq!(resolve(&cfg, "/work/ULIX/repo"), Some("openai-1".into()));
    }
    #[test]
    fn default_applies_outside_every_rule() {
        let cfg = config_with(Some("openai-2"), vec![("/work/ULIX", "openai-1")]);
        assert_eq!(resolve(&cfg, "/work/other"), Some("openai-2".into()));
    }
    #[test]
    fn rules_may_name_an_email_instead_of_a_positional_label() {
        // The stable, recommended form: labels are positional and shift when
        // accounts are added or removed, emails do not.
        let cfg = config_with(
            Some("daniil.kovaliov@gmail.com"),
            vec![("/work/ULIX", "it@ulixtravel.com")],
        );
        assert_eq!(resolve(&cfg, "/work/ULIX/x"), Some("openai-1".into()));
        assert_eq!(resolve(&cfg, "/elsewhere"), Some("openai-2".into()));
    }
    #[test]
    fn rules_may_name_an_account_id() {
        let cfg = config_with(None, vec![("/work/ULIX", "7eb515b8")]);
        assert_eq!(resolve(&cfg, "/work/ULIX"), Some("openai-1".into()));
    }
    #[test]
    fn email_matching_is_case_insensitive() {
        let cfg = config_with(None, vec![("/work/ULIX", "IT@UlixTravel.com")]);
        assert_eq!(resolve(&cfg, "/work/ULIX"), Some("openai-1".into()));
    }
    #[test]
    fn account_id_matching_is_case_sensitive() {
        // Account ids are opaque identifiers, so do not fuzz them.
        let cfg = config_with(None, vec![("/work/ULIX", "7EB515B8")]);
        assert_eq!(resolve(&cfg, "/work/ULIX"), None);
    }
    #[test]
    fn missing_rule_account_falls_back_to_default_not_silently_to_the_rule() {
        // Guards the worst failure: a typo'd company account must not quietly
        // send company work to the personal subscription without a warning.
        let cfg = config_with(Some("openai-2"), vec![("/work/ULIX", "openai-typo")]);
        assert_eq!(resolve(&cfg, "/work/ULIX"), Some("openai-2".into()));
    }
    #[test]
    fn missing_rule_and_missing_default_yields_none() {
        let cfg = config_with(Some("openai-gone"), vec![("/work/ULIX", "openai-typo")]);
        assert_eq!(resolve(&cfg, "/work/ULIX"), None);
    }
    #[test]
    fn missing_default_yields_none_so_persisted_active_applies() {
        let cfg = config_with(Some("nobody@example.com"), vec![]);
        assert_eq!(resolve(&cfg, "/anywhere"), None);
    }
    #[test]
    fn tilde_paths_expand_to_the_user_home() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let cfg = config_with(Some("openai-2"), vec![("~/work/ULIX", "openai-1")]);
        let dir = home.join("work/ULIX/src");
        assert_eq!(
            resolve(&cfg, &dir.to_string_lossy()),
            Some("openai-1".into())
        );
    }
    #[test]
    fn expand_home_leaves_absolute_paths_alone() {
        assert_eq!(expand_home("/absolute/path"), "/absolute/path");
    }
    #[test]
    fn construction_pin_is_visible_then_cleared_on_drop() {
        assert_eq!(pinned_account_for_construction("openai"), None);
        {
            let _guard = pin_account_for_construction("openai", "openai-1".into());
            assert_eq!(
                pinned_account_for_construction("openai"),
                Some("openai-1".into())
            );
        }
        assert_eq!(pinned_account_for_construction("openai"), None);
    }

    #[test]
    fn nested_construction_pins_restore_the_outer_value() {
        let _outer = pin_account_for_construction("openai", "openai-1".into());
        {
            let _inner = pin_account_for_construction("openai", "openai-2".into());
            assert_eq!(
                pinned_account_for_construction("openai"),
                Some("openai-2".into())
            );
        }
        assert_eq!(
            pinned_account_for_construction("openai"),
            Some("openai-1".into())
        );
    }

    #[test]
    fn construction_pins_are_isolated_per_provider() {
        let _openai = pin_account_for_construction("openai", "openai-1".into());
        assert_eq!(pinned_account_for_construction("claude"), None);
    }

    #[test]
    fn construction_pins_do_not_leak_across_threads() {
        // Two sessions can be created concurrently; one must never observe the
        // other's pinned subscription.
        let _guard = pin_account_for_construction("openai", "openai-1".into());
        let observed = std::thread::spawn(|| pinned_account_for_construction("openai"))
            .join()
            .expect("thread");
        assert_eq!(observed, None);
    }

    #[test]
    fn rule_bound_accounts_are_reserved() {
        let cfg = config_with(
            Some("daniil.kovaliov@gmail.com"),
            vec![("~/Documents/LeGrin.tech/ULIX", "it@ulixtravel.com")],
        );
        // The rule names an email; reservation is reported as a label.
        assert_eq!(
            reserved_labels_with_config(&cfg, &accounts()),
            vec!["openai-1".to_string()]
        );
    }

    #[test]
    fn default_only_account_is_not_reserved() {
        // The default is the intended catch-all, so it stays a legal failover
        // target; only path-bound accounts are ring-fenced.
        let cfg = config_with(Some("daniil.kovaliov@gmail.com"), vec![]);
        assert!(reserved_labels_with_config(&cfg, &accounts()).is_empty());
    }

    #[test]
    fn reserved_labels_deduplicate_across_rules() {
        let cfg = config_with(
            None,
            vec![
                ("~/Documents/LeGrin.tech/ULIX", "it@ulixtravel.com"),
                ("~/work/ulix-extra", "openai-1"),
                ("~/work/other", "7eb515b8"),
            ],
        );
        assert_eq!(
            reserved_labels_with_config(&cfg, &accounts()),
            vec!["openai-1".to_string()]
        );
    }

    #[test]
    fn rules_naming_unknown_accounts_reserve_nothing() {
        // A stale rule must not ring-fence an account that is not logged in,
        // which would otherwise disable failover for no reason.
        let cfg = config_with(None, vec![("~/work", "someone@example.com")]);
        assert!(reserved_labels_with_config(&cfg, &accounts()).is_empty());
    }

    #[test]
    fn display_name_prefers_email_local_part() {
        // The positional label (openai-1) is meaningless to a human; the point
        // of the badge is to say *which subscription pays*.
        let accts = accounts();
        let company = accts.iter().find(|a| a.label == "openai-1").unwrap();
        let shown = match company.email.as_deref() {
            Some(email) => email.split('@').next().unwrap_or(email).to_string(),
            None => company.label.clone(),
        };
        assert_eq!(shown, "it");
    }

    #[test]
    fn display_name_falls_back_to_label_without_email() {
        let account = AccountIdentity::new("openai-3", None, Some("x".into()));
        let shown = match account.email.as_deref() {
            Some(email) => email.split('@').next().unwrap_or(email).to_string(),
            None => account.label.clone(),
        };
        assert_eq!(shown, "openai-3");
    }
}
