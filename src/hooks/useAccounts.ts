import { useState, useEffect, useCallback, useRef } from "react";
import type {
  AccountInfo,
  UsageInfo,
  AccountWithUsage,
  CachedUsageInfo,
  WarmupSummary,
  ImportAccountsSummary,
  LiveAuthSyncResult,
} from "../types";
import { invokeBackend, type FileSource } from "../lib/platform";
import {
  applyUsageFetchError,
  applyUsageFetchResult,
  filterCachedUsageEntries,
  loadCachedUsageFromBrowser,
  markAccountsUsageLoading,
  mergeCachedUsageEntries,
  mergeAccountsWithCachedUsage,
  persistCachedUsageToBrowser,
  saveCachedUsageToBrowser,
} from "../lib/usageCache";

export function useAccounts() {
  const [accounts, setAccounts] = useState<AccountWithUsage[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const accountsRef = useRef<AccountWithUsage[]>([]);
  const maxConcurrentUsageRequests = 10;

  useEffect(() => {
    accountsRef.current = accounts;
  }, [accounts]);

  const buildUsageError = useCallback(
    (accountId: string, message: string, planType: string | null): UsageInfo => ({
      account_id: accountId,
      plan_type: planType,
      primary_used_percent: null,
      primary_window_minutes: null,
      primary_resets_at: null,
      secondary_used_percent: null,
      secondary_window_minutes: null,
      secondary_resets_at: null,
      has_credits: null,
      unlimited_credits: null,
      credits_balance: null,
      error: message,
    }),
    []
  );

  const runWithConcurrency = useCallback(
    async <T,>(
      items: T[],
      worker: (item: T) => Promise<void>,
      concurrency: number
    ) => {
      if (items.length === 0) return;
      const limit = Math.min(Math.max(concurrency, 1), items.length);
      let index = 0;
      const runners = Array.from({ length: limit }, async () => {
        while (true) {
          const current = index++;
          if (current >= items.length) return;
          await worker(items[current]);
        }
      });
      await Promise.allSettled(runners);
    },
    []
  );

  const loadAccounts = useCallback(async (preserveUsage = false) => {
    try {
      setLoading(true);
      setError(null);
      const browserCachedUsage = loadCachedUsageFromBrowser();
      const [accountList, cachedUsage] = await Promise.all([
        invokeBackend<AccountInfo[]>("list_accounts"),
        invokeBackend<CachedUsageInfo[]>("get_cached_usage").catch((err) => {
          console.error("Failed to load cached usage:", err);
          return [];
        }),
      ]);
      const accountIdSet = new Set(accountList.map((account) => account.id));
      const mergedCachedUsage = filterCachedUsageEntries(
        mergeCachedUsageEntries(cachedUsage, browserCachedUsage),
        accountIdSet
      );

      persistCachedUsageToBrowser(mergedCachedUsage);

      setAccounts((prev) =>
        mergeAccountsWithCachedUsage(accountList, prev, mergedCachedUsage, preserveUsage)
      );
      return accountList;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      return [];
    } finally {
      setLoading(false);
    }
  }, []);

  const syncLiveAuth = useCallback(
    async (reloadOnChange = true) => {
      try {
        const result = await invokeBackend<LiveAuthSyncResult>("sync_live_auth");
        if (reloadOnChange && result.changed) {
          return await loadAccounts(true);
        }
        return null;
      } catch (err) {
        console.error("Failed to sync live auth:", err);
        return null;
      }
    },
    [loadAccounts]
  );

  const refreshUsage = useCallback(
    async (accountList?: AccountInfo[] | AccountWithUsage[]) => {
      try {
        const list = accountList ?? accountsRef.current;
        if (list.length === 0) {
          return;
        }

        const accountIds = list.map((account) => account.id);
        const accountIdSet = new Set(accountIds);

        setAccounts((prev) => markAccountsUsageLoading(prev, accountIdSet));

        await runWithConcurrency(
          accountIds,
          async (accountId) => {
            try {
              const usage = await invokeBackend<UsageInfo>("get_usage", {
                accountId,
                account_id: accountId,
              });
              const updatedAt = new Date().toISOString();
              if (!usage.error) {
                saveCachedUsageToBrowser({
                  account_id: accountId,
                  usage,
                  updated_at: updatedAt,
                });
              }
              setAccounts((prev) => applyUsageFetchResult(prev, accountId, usage, updatedAt));
            } catch (err) {
              console.error("Failed to refresh usage:", err);
              const message = err instanceof Error ? err.message : String(err);
              setAccounts((prev) =>
                applyUsageFetchError(
                  prev,
                  accountId,
                  buildUsageError(
                    accountId,
                    message,
                    prev.find((account) => account.id === accountId)?.plan_type ?? null
                  )
                )
              );
            }
          },
          maxConcurrentUsageRequests
        );
      } catch (err) {
        console.error("Failed to refresh usage:", err);
        throw err;
      }
    },
    [buildUsageError, maxConcurrentUsageRequests, runWithConcurrency]
  );

  const refreshSingleUsage = useCallback(async (accountId: string) => {
    try {
      setAccounts((prev) =>
        prev.map((a) =>
          a.id === accountId ? { ...a, usageLoading: true } : a
        )
      );
      const usage = await invokeBackend<UsageInfo>("get_usage", {
        accountId,
        account_id: accountId,
      });
      const updatedAt = new Date().toISOString();
      if (!usage.error) {
        saveCachedUsageToBrowser({
          account_id: accountId,
          usage,
          updated_at: updatedAt,
        });
      }
      setAccounts((prev) => applyUsageFetchResult(prev, accountId, usage, updatedAt));
    } catch (err) {
      console.error("Failed to refresh single usage:", err);
      const message = err instanceof Error ? err.message : String(err);
      setAccounts((prev) =>
        applyUsageFetchError(
          prev,
          accountId,
          buildUsageError(
            accountId,
            message,
            prev.find((account) => account.id === accountId)?.plan_type ?? null
          )
        )
      );
      throw err;
    }
  }, [buildUsageError]);

  const warmupAccount = useCallback(async (accountId: string) => {
    try {
      await invokeBackend("warmup_account", { accountId, account_id: accountId });
    } catch (err) {
      console.error("Failed to warm up account:", err);
      throw err;
    }
  }, []);

  const warmupAllAccounts = useCallback(async () => {
    try {
      return await invokeBackend<WarmupSummary>("warmup_all_accounts");
    } catch (err) {
      console.error("Failed to warm up all accounts:", err);
      throw err;
    }
  }, []);

  const switchAccount = useCallback(
    async (accountId: string) => {
      try {
        await invokeBackend("switch_account", { accountId, account_id: accountId });
        await loadAccounts(true); // Preserve usage data
      } catch (err) {
        throw err;
      }
    },
    [loadAccounts]
  );

  const deleteAccount = useCallback(
    async (accountId: string) => {
      try {
        await invokeBackend("delete_account", { accountId, account_id: accountId });
        await loadAccounts();
      } catch (err) {
        throw err;
      }
    },
    [loadAccounts]
  );

  const renameAccount = useCallback(
    async (accountId: string, newName: string) => {
      try {
        await invokeBackend("rename_account", {
          accountId,
          account_id: accountId,
          newName,
          new_name: newName,
        });
        await loadAccounts(true); // Preserve usage data
      } catch (err) {
        throw err;
      }
    },
    [loadAccounts]
  );

  const importFromFile = useCallback(
    async (source: FileSource, name: string) => {
      try {
        if (typeof source === "string") {
          await invokeBackend<AccountInfo>("add_account_from_file", { path: source, name });
        } else {
          const contents = await source.text();
          await invokeBackend<AccountInfo>("add_account_from_auth_json_text", {
            name,
            contents,
          });
        }
        const accountList = await loadAccounts();
        await refreshUsage(accountList);
      } catch (err) {
        throw err;
      }
    },
    [loadAccounts, refreshUsage]
  );

  const startOAuthLogin = useCallback(async (accountName: string) => {
    try {
      const info = await invokeBackend<{ auth_url: string; callback_port: number }>(
        "start_login",
        { accountName, account_name: accountName }
      );
      return info;
    } catch (err) {
      throw err;
    }
  }, []);

  const completeOAuthLogin = useCallback(async () => {
    try {
      const account = await invokeBackend<AccountInfo>("complete_login");
      const accountList = await loadAccounts();
      await refreshUsage(accountList);
      return account;
    } catch (err) {
      throw err;
    }
  }, [loadAccounts, refreshUsage]);

  const exportAccountsSlimText = useCallback(async () => {
    try {
      return await invokeBackend<string>("export_accounts_slim_text");
    } catch (err) {
      throw err;
    }
  }, []);

  const importAccountsSlimText = useCallback(
    async (payload: string) => {
      try {
        const summary = await invokeBackend<ImportAccountsSummary>("import_accounts_slim_text", {
          payload,
        });
        const accountList = await loadAccounts();
        await refreshUsage(accountList);
        return summary;
      } catch (err) {
        throw err;
      }
    },
    [loadAccounts, refreshUsage]
  );

  const exportAccountsFullEncryptedFile = useCallback(
    async (path: string) => {
      try {
        await invokeBackend("export_accounts_full_encrypted_file", { path });
      } catch (err) {
        throw err;
      }
    },
    []
  );

  const importAccountsFullEncryptedFile = useCallback(
    async (path: string) => {
      try {
        const summary = await invokeBackend<ImportAccountsSummary>(
          "import_accounts_full_encrypted_file",
          { path }
        );
        const accountList = await loadAccounts();
        await refreshUsage(accountList);
        return summary;
      } catch (err) {
        throw err;
      }
    },
    [loadAccounts, refreshUsage]
  );

  const cancelOAuthLogin = useCallback(async () => {
    try {
      await invokeBackend("cancel_login");
    } catch (err) {
      console.error("Failed to cancel login:", err);
    }
  }, []);

  const loadMaskedAccountIds = useCallback(async () => {
    try {
      return await invokeBackend<string[]>("get_masked_account_ids");
    } catch (err) {
      console.error("Failed to load masked account IDs:", err);
      return [];
    }
  }, []);

  const saveMaskedAccountIds = useCallback(async (ids: string[]) => {
    try {
      await invokeBackend("set_masked_account_ids", { ids });
    } catch (err) {
      console.error("Failed to save masked account IDs:", err);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;

    const initialize = async () => {
      await syncLiveAuth(false);
      if (cancelled) return;

      const accountList = await loadAccounts();
      if (cancelled) return;

      await refreshUsage(accountList);
    };

    void initialize();

    const syncInterval = setInterval(() => {
      syncLiveAuth(true).catch(() => {});
    }, 5000);

    const usageInterval = setInterval(() => {
      refreshUsage().catch(() => {});
    }, 60000);

    const handleWindowFocus = () => {
      syncLiveAuth(true).catch(() => {});
    };

    window.addEventListener("focus", handleWindowFocus);

    return () => {
      cancelled = true;
      clearInterval(syncInterval);
      clearInterval(usageInterval);
      window.removeEventListener("focus", handleWindowFocus);
    };
  }, [loadAccounts, refreshUsage, syncLiveAuth]);

  return {
    accounts,
    loading,
    error,
    loadAccounts,
    syncLiveAuth,
    refreshUsage,
    refreshSingleUsage,
    warmupAccount,
    warmupAllAccounts,
    switchAccount,
    deleteAccount,
    renameAccount,
    importFromFile,
    exportAccountsSlimText,
    importAccountsSlimText,
    exportAccountsFullEncryptedFile,
    importAccountsFullEncryptedFile,
    startOAuthLogin,
    completeOAuthLogin,
    cancelOAuthLogin,
    loadMaskedAccountIds,
    saveMaskedAccountIds,
  };
}
