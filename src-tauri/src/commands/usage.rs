//! Usage query Tauri commands

use crate::api::usage::{get_account_usage, refresh_all_usage, warmup_account as send_warmup};
use crate::auth::{
    cache_usage, cache_usages, get_account, load_accounts, load_cached_usage_for_account_ids,
};
use crate::types::{CachedUsageEntry, UsageInfo, WarmupSummary};
use futures::{stream, StreamExt};

/// Get usage info for a specific account
#[tauri::command]
pub async fn get_usage(account_id: String) -> Result<UsageInfo, String> {
    let account = get_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Account not found: {account_id}"))?;

    let usage = get_account_usage(&account)
        .await
        .map_err(|e| e.to_string())?;
    if usage.error.is_none() {
        if let Err(error) = cache_usage(&usage) {
            eprintln!(
                "[Usage] Failed to cache usage for {}: {error:#}",
                account.id
            );
        }
    }

    Ok(usage)
}

/// Refresh usage info for all accounts
#[tauri::command]
pub async fn refresh_all_accounts_usage() -> Result<Vec<UsageInfo>, String> {
    let store = load_accounts().map_err(|e| e.to_string())?;
    let usages = refresh_all_usage(&store.accounts).await;
    if let Err(error) = cache_usages(&usages) {
        eprintln!("[Usage] Failed to cache bulk usage refresh: {error:#}");
    }

    Ok(usages)
}

/// Load the last successful usage entries from disk.
#[tauri::command]
pub async fn get_cached_usage() -> Result<Vec<CachedUsageEntry>, String> {
    let store = load_accounts().map_err(|e| e.to_string())?;
    let account_ids = store
        .accounts
        .into_iter()
        .map(|account| account.id)
        .collect::<Vec<_>>();
    load_cached_usage_for_account_ids(&account_ids).map_err(|e| e.to_string())
}

/// Send a minimal warm-up request for one account
#[tauri::command]
pub async fn warmup_account(account_id: String) -> Result<(), String> {
    let account = get_account(&account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Account not found: {account_id}"))?;

    send_warmup(&account).await.map_err(|e| e.to_string())
}

/// Send minimal warm-up requests for all accounts
#[tauri::command]
pub async fn warmup_all_accounts() -> Result<WarmupSummary, String> {
    let store = load_accounts().map_err(|e| e.to_string())?;
    let total_accounts = store.accounts.len();
    let concurrency = total_accounts.min(10).max(1);

    let results: Vec<(String, bool)> = stream::iter(store.accounts.into_iter())
        .map(|account| async move {
            let account_id = account.id.clone();
            let failed = send_warmup(&account).await.is_err();
            (account_id, failed)
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;

    let failed_account_ids = results
        .into_iter()
        .filter_map(|(account_id, failed)| failed.then_some(account_id))
        .collect::<Vec<_>>();

    let warmed_accounts = total_accounts.saturating_sub(failed_account_ids.len());
    Ok(WarmupSummary {
        total_accounts,
        warmed_accounts,
        failed_account_ids,
    })
}
