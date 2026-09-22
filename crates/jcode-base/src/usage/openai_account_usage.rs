use super::{
    OpenAIUsageData, auth, cached_openai_usage, fetch_openai_usage_for_account,
    openai_provider_display_name, openai_usage_cache_key,
};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static REFRESHING_LABELS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static ACCOUNT_CACHE: OnceLock<Mutex<Option<CachedAccounts>>> = OnceLock::new();
const ACCOUNT_CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct CachedAccounts {
    fetched_at: Instant,
    active_label: Option<String>,
    accounts: Vec<auth::codex::OpenAiAccount>,
}

fn refreshing_labels() -> &'static Mutex<HashSet<String>> {
    REFRESHING_LABELS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn cached_accounts() -> Option<CachedAccounts> {
    let cache = ACCOUNT_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(cache) = cache.lock()
        && let Some(snapshot) = cache.as_ref()
        && snapshot.fetched_at.elapsed() < ACCOUNT_CACHE_TTL
    {
        return Some(snapshot.clone());
    }

    let accounts = auth::codex::list_accounts().ok()?;
    let snapshot = CachedAccounts {
        fetched_at: Instant::now(),
        active_label: auth::codex::active_account_label(),
        accounts,
    };
    if let Ok(mut cache) = cache.lock() {
        *cache = Some(snapshot.clone());
    }
    Some(snapshot)
}

fn selected_account(
    accounts: &[auth::codex::OpenAiAccount],
    requested_label: Option<&str>,
    active_label: Option<&str>,
) -> Option<auth::codex::OpenAiAccount> {
    let label = requested_label
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .or_else(|| active_label.map(str::to_owned))?;
    accounts
        .iter()
        .find(|account| account.label == label)
        .cloned()
        .or_else(|| accounts.first().cloned())
}

fn bound_label_for_working_dir(
    working_dir: Option<&str>,
    accounts: &[auth::codex::OpenAiAccount],
) -> Option<String> {
    let working_dir = working_dir?.trim();
    if working_dir.is_empty() {
        return None;
    }
    let identities = accounts
        .iter()
        .map(|account| {
            auth::account_binding::AccountIdentity::new(
                account.label.clone(),
                account.email.clone(),
                account.account_id.clone(),
            )
        })
        .collect::<Vec<_>>();
    let config = crate::config::config();
    let provider_config = config.accounts.provider("openai")?;
    auth::account_binding::resolve_label_with_config(
        provider_config,
        "openai",
        working_dir,
        &identities,
    )
}

fn account_display_label(account: &auth::codex::OpenAiAccount) -> String {
    account
        .email
        .as_deref()
        .map(super::mask_email)
        .unwrap_or_else(|| account.label.clone())
}

fn account_usage_snapshot(account: &auth::codex::OpenAiAccount) -> OpenAIUsageData {
    let cache_key = openai_usage_cache_key(&account.access_token, Some(&account.label));
    let mut usage = cached_openai_usage(&cache_key).unwrap_or_default();
    usage.account_label = Some(account_display_label(account));
    usage
}

fn spawn_refresh(account: auth::codex::OpenAiAccount) {
    let label = account.label.clone();
    let Ok(mut refreshing) = refreshing_labels().lock() else {
        return;
    };
    if !refreshing.insert(label.clone()) {
        return;
    }
    drop(refreshing);

    tokio::spawn(async move {
        let credentials = auth::codex::CodexCredentials {
            access_token: account.access_token.clone(),
            refresh_token: account.refresh_token.clone(),
            id_token: account.id_token.clone(),
            account_id: account.account_id.clone(),
            expires_at: account.expires_at,
        };
        let _ = fetch_openai_usage_for_account(
            openai_provider_display_name(&account.label, account.email.as_deref(), 2, false),
            credentials,
            Some(&account.label),
        )
        .await;
        if let Ok(mut refreshing) = refreshing_labels().lock() {
            refreshing.remove(&label);
        }
    });
}

/// Return the cached quota for the account the current session is bound to.
/// Rendering never blocks: a cache miss schedules one background refresh for
/// that account and returns an identity-tagged empty snapshot until it lands.
pub fn get_openai_usage_for_working_dir_sync(working_dir: Option<&str>) -> OpenAIUsageData {
    let Some(snapshot) = cached_accounts() else {
        return OpenAIUsageData::default();
    };
    let requested_label = bound_label_for_working_dir(working_dir, &snapshot.accounts);
    let Some(account) = selected_account(
        &snapshot.accounts,
        requested_label.as_deref(),
        snapshot.active_label.as_deref(),
    ) else {
        return OpenAIUsageData::default();
    };

    let usage = account_usage_snapshot(&account);
    if usage.fetched_at.is_none() && tokio::runtime::Handle::try_current().is_ok() {
        spawn_refresh(account);
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(label: &str) -> auth::codex::OpenAiAccount {
        auth::codex::OpenAiAccount {
            label: label.to_string(),
            access_token: format!("{label}-access"),
            refresh_token: String::new(),
            id_token: None,
            account_id: None,
            expires_at: None,
            email: None,
        }
    }

    #[test]
    fn bound_account_wins_over_the_process_global_selection() {
        let accounts = vec![account("company"), account("personal")];

        let selected = selected_account(&accounts, Some("personal"), Some("company"))
            .expect("selected account");

        assert_eq!(selected.label, "personal");
    }
}
