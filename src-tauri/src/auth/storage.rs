//! Account storage module - manages reading and writing accounts.json

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::types::{
    AccountsStore, AuthData, CachedUsageEntry, StoredAccount, UsageCacheStore, UsageInfo,
};

/// Get the path to the codex-switcher config directory
pub fn get_config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not find home directory")?;
    Ok(home.join(".codex-switcher"))
}

/// Get the path to accounts.json
pub fn get_accounts_file() -> Result<PathBuf> {
    Ok(get_config_dir()?.join("accounts.json"))
}

/// Get the path to usage-cache.json
pub fn get_usage_cache_file() -> Result<PathBuf> {
    Ok(get_config_dir()?.join("usage-cache.json"))
}

/// Load the accounts store from disk
pub fn load_accounts() -> Result<AccountsStore> {
    let path = get_accounts_file()?;

    if !path.exists() {
        return Ok(AccountsStore::default());
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read accounts file: {}", path.display()))?;

    let store: AccountsStore = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse accounts file: {}", path.display()))?;

    Ok(store)
}

/// Save the accounts store to disk
pub fn save_accounts(store: &AccountsStore) -> Result<()> {
    let path = get_accounts_file()?;

    // Ensure the config directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    let content =
        serde_json::to_string_pretty(store).context("Failed to serialize accounts store")?;

    fs::write(&path, content)
        .with_context(|| format!("Failed to write accounts file: {}", path.display()))?;

    // Set restrictive permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

/// Load the usage cache store from disk.
pub fn load_usage_cache() -> Result<UsageCacheStore> {
    let path = get_usage_cache_file()?;

    if !path.exists() {
        return Ok(UsageCacheStore::default());
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read usage cache file: {}", path.display()))?;

    let store: UsageCacheStore = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse usage cache file: {}", path.display()))?;

    Ok(store)
}

/// Save the usage cache store to disk.
pub fn save_usage_cache(store: &UsageCacheStore) -> Result<()> {
    let path = get_usage_cache_file()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    let content = serde_json::to_string_pretty(store).context("Failed to serialize usage cache")?;

    fs::write(&path, content)
        .with_context(|| format!("Failed to write usage cache file: {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

/// Load cached usage entries and prune accounts that no longer exist.
pub fn load_cached_usage_for_account_ids(account_ids: &[String]) -> Result<Vec<CachedUsageEntry>> {
    let mut store = load_usage_cache()?;
    let valid_ids = account_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let initial_len = store.entries.len();

    store
        .entries
        .retain(|entry| valid_ids.contains(entry.account_id.as_str()));

    if store.entries.len() != initial_len {
        save_usage_cache(&store)?;
    }

    Ok(store.entries)
}

/// Store the latest successful usage payload for one account.
pub fn cache_usage(usage: &UsageInfo) -> Result<CachedUsageEntry> {
    let mut store = load_usage_cache()?;
    let cached = CachedUsageEntry {
        account_id: usage.account_id.clone(),
        usage: usage.clone(),
        updated_at: Utc::now(),
    };

    if let Some(existing) = store
        .entries
        .iter_mut()
        .find(|entry| entry.account_id == cached.account_id)
    {
        *existing = cached.clone();
    } else {
        store.entries.push(cached.clone());
    }

    store
        .entries
        .sort_by(|left, right| left.account_id.cmp(&right.account_id));
    save_usage_cache(&store)?;

    Ok(cached)
}

/// Store the latest successful usage payloads for many accounts in one write.
pub fn cache_usages(usages: &[UsageInfo]) -> Result<()> {
    let successful = usages
        .iter()
        .filter(|usage| usage.error.is_none())
        .collect::<Vec<_>>();

    if successful.is_empty() {
        return Ok(());
    }

    let mut store = load_usage_cache()?;

    for usage in successful {
        let cached = CachedUsageEntry {
            account_id: usage.account_id.clone(),
            usage: usage.clone(),
            updated_at: Utc::now(),
        };

        if let Some(existing) = store
            .entries
            .iter_mut()
            .find(|entry| entry.account_id == cached.account_id)
        {
            *existing = cached;
        } else {
            store.entries.push(cached);
        }
    }

    store
        .entries
        .sort_by(|left, right| left.account_id.cmp(&right.account_id));
    save_usage_cache(&store)
}

/// Add a new account to the store
pub fn add_account(account: StoredAccount) -> Result<StoredAccount> {
    let mut store = load_accounts()?;

    // Check for duplicate names
    if store.accounts.iter().any(|a| a.name == account.name) {
        anyhow::bail!("An account with name '{}' already exists", account.name);
    }

    let account_clone = account.clone();
    store.accounts.push(account);

    // If this is the first account, make it active
    if store.accounts.len() == 1 {
        store.active_account_id = Some(account_clone.id.clone());
    }

    save_accounts(&store)?;
    Ok(account_clone)
}

/// Remove an account by ID
pub fn remove_account(account_id: &str) -> Result<()> {
    let mut store = load_accounts()?;

    let initial_len = store.accounts.len();
    store.accounts.retain(|a| a.id != account_id);

    if store.accounts.len() == initial_len {
        anyhow::bail!("Account not found: {account_id}");
    }

    // If we removed the active account, clear it or set to first available
    if store.active_account_id.as_deref() == Some(account_id) {
        store.active_account_id = store.accounts.first().map(|a| a.id.clone());
    }

    save_accounts(&store)?;
    Ok(())
}

/// Update the active account ID
pub fn set_active_account(account_id: &str) -> Result<()> {
    let mut store = load_accounts()?;

    // Verify the account exists
    if !store.accounts.iter().any(|a| a.id == account_id) {
        anyhow::bail!("Account not found: {account_id}");
    }

    store.active_account_id = Some(account_id.to_string());
    save_accounts(&store)?;
    Ok(())
}

/// Get an account by ID
pub fn get_account(account_id: &str) -> Result<Option<StoredAccount>> {
    let store = load_accounts()?;
    Ok(store.accounts.into_iter().find(|a| a.id == account_id))
}

/// Get the currently active account
pub fn get_active_account() -> Result<Option<StoredAccount>> {
    let store = load_accounts()?;
    let active_id = match &store.active_account_id {
        Some(id) => id,
        None => return Ok(None),
    };
    Ok(store.accounts.into_iter().find(|a| a.id == *active_id))
}

/// Update an account's last_used_at timestamp
pub fn touch_account(account_id: &str) -> Result<()> {
    let mut store = load_accounts()?;

    if let Some(account) = store.accounts.iter_mut().find(|a| a.id == account_id) {
        account.last_used_at = Some(chrono::Utc::now());
        save_accounts(&store)?;
    }

    Ok(())
}

/// Update an account's metadata (name, email, plan_type)
pub fn update_account_metadata(
    account_id: &str,
    name: Option<String>,
    email: Option<String>,
    plan_type: Option<String>,
) -> Result<()> {
    let mut store = load_accounts()?;

    // Check for duplicate names first (if renaming)
    if let Some(ref new_name) = name {
        if store
            .accounts
            .iter()
            .any(|a| a.id != account_id && a.name == *new_name)
        {
            anyhow::bail!("An account with name '{new_name}' already exists");
        }
    }

    // Now find and update the account
    let account = store
        .accounts
        .iter_mut()
        .find(|a| a.id == account_id)
        .context("Account not found")?;

    if let Some(new_name) = name {
        account.name = new_name;
    }

    if email.is_some() {
        account.email = email;
    }

    if plan_type.is_some() {
        account.plan_type = plan_type;
    }

    save_accounts(&store)?;
    Ok(())
}

/// Update ChatGPT OAuth tokens for an account and return the updated account.
pub fn update_account_chatgpt_tokens(
    account_id: &str,
    id_token: String,
    access_token: String,
    refresh_token: String,
    chatgpt_account_id: Option<String>,
    email: Option<String>,
    plan_type: Option<String>,
) -> Result<StoredAccount> {
    let mut store = load_accounts()?;

    let account = store
        .accounts
        .iter_mut()
        .find(|a| a.id == account_id)
        .context("Account not found")?;

    match &mut account.auth_data {
        AuthData::ChatGPT {
            id_token: stored_id_token,
            access_token: stored_access_token,
            refresh_token: stored_refresh_token,
            account_id: stored_account_id,
        } => {
            *stored_id_token = id_token;
            *stored_access_token = access_token;
            *stored_refresh_token = refresh_token;
            if let Some(new_account_id) = chatgpt_account_id {
                *stored_account_id = Some(new_account_id);
            }
        }
        AuthData::ApiKey { .. } => {
            anyhow::bail!("Cannot update OAuth tokens for an API key account");
        }
    }

    if let Some(new_email) = email {
        account.email = Some(new_email);
    }

    if let Some(new_plan_type) = plan_type {
        account.plan_type = Some(new_plan_type);
    }

    let updated = account.clone();
    save_accounts(&store)?;
    Ok(updated)
}

/// Get the list of masked account IDs
pub fn get_masked_account_ids() -> Result<Vec<String>> {
    let store = load_accounts()?;
    Ok(store.masked_account_ids.clone())
}

/// Set the list of masked account IDs
pub fn set_masked_account_ids(ids: Vec<String>) -> Result<()> {
    let mut store = load_accounts()?;
    store.masked_account_ids = ids;
    save_accounts(&store)?;
    Ok(())
}
