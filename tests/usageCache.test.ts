import assert from "node:assert/strict";
import test from "node:test";

import {
  applyUsageFetchError,
  applyUsageFetchResult,
  mergeCachedUsageEntries,
  mergeAccountsWithCachedUsage,
  upsertCachedUsageEntry,
} from "../src/lib/usageCache";
import type { AccountInfo, AccountWithUsage, CachedUsageInfo, UsageInfo } from "../src/types";

function makeAccount(id: string, name = id): AccountInfo {
  return {
    id,
    name,
    email: `${name}@example.com`,
    plan_type: "plus",
    auth_mode: "chat_gpt",
    is_active: false,
    created_at: "2026-03-22T00:00:00.000Z",
    last_used_at: null,
  };
}

function makeUsage(accountId: string, primaryUsedPercent = 25): UsageInfo {
  return {
    account_id: accountId,
    plan_type: "plus",
    primary_used_percent: primaryUsedPercent,
    primary_window_minutes: 300,
    primary_resets_at: 1774180558,
    secondary_used_percent: 10,
    secondary_window_minutes: 10080,
    secondary_resets_at: 1774580558,
    has_credits: false,
    unlimited_credits: false,
    credits_balance: "0",
    error: null,
  };
}

function makeCachedUsage(accountId: string, updatedAt: string): CachedUsageInfo {
  return {
    account_id: accountId,
    usage: makeUsage(accountId),
    updated_at: updatedAt,
  };
}

test("mergeAccountsWithCachedUsage seeds cached usage for cold start", () => {
  const accounts = [makeAccount("alpha"), makeAccount("beta")];
  const cached = [makeCachedUsage("alpha", "2026-03-25T00:10:00.000Z")];

  const merged = mergeAccountsWithCachedUsage(accounts, [], cached, false);

  assert.deepEqual(merged[0].usage, cached[0].usage);
  assert.equal(merged[0].usageUpdatedAt, "2026-03-25T00:10:00.000Z");
  assert.equal(merged[0].usageLoading, false);
  assert.equal(merged[1].usage, undefined);
  assert.equal(merged[1].usageUpdatedAt, null);
});

test("mergeAccountsWithCachedUsage preserves live usage but backfills cache for new accounts", () => {
  const accounts = [makeAccount("alpha"), makeAccount("beta")];
  const previous: AccountWithUsage[] = [
    {
      ...accounts[0],
      usage: makeUsage("alpha", 70),
      usageLoading: true,
      usageUpdatedAt: "2026-03-25T00:20:00.000Z",
    },
  ];
  const cached = [makeCachedUsage("beta", "2026-03-25T00:15:00.000Z")];

  const merged = mergeAccountsWithCachedUsage(accounts, previous, cached, true);

  assert.equal(merged[0].usage?.primary_used_percent, 70);
  assert.equal(merged[0].usageLoading, true);
  assert.equal(merged[0].usageUpdatedAt, "2026-03-25T00:20:00.000Z");
  assert.deepEqual(merged[1].usage, cached[0].usage);
  assert.equal(merged[1].usageUpdatedAt, "2026-03-25T00:15:00.000Z");
});

test("mergeCachedUsageEntries keeps the newest entry per account across backend and browser cache", () => {
  const merged = mergeCachedUsageEntries(
    [makeCachedUsage("alpha", "2026-03-25T00:20:00.000Z")],
    [
      makeCachedUsage("alpha", "2026-03-25T00:10:00.000Z"),
      makeCachedUsage("beta", "2026-03-25T00:15:00.000Z"),
    ]
  );

  assert.equal(merged.length, 2);
  assert.equal(merged[0].account_id, "alpha");
  assert.equal(merged[0].updated_at, "2026-03-25T00:20:00.000Z");
  assert.equal(merged[1].account_id, "beta");
});

test("upsertCachedUsageEntry replaces the existing account cache entry", () => {
  const updated = upsertCachedUsageEntry(
    [
      makeCachedUsage("alpha", "2026-03-25T00:10:00.000Z"),
      makeCachedUsage("beta", "2026-03-25T00:15:00.000Z"),
    ],
    makeCachedUsage("alpha", "2026-03-25T00:30:00.000Z")
  );

  assert.equal(updated.length, 2);
  assert.equal(updated[0].account_id, "alpha");
  assert.equal(updated[0].updated_at, "2026-03-25T00:30:00.000Z");
});

test("applyUsageFetchResult updates usage and timestamp after a successful refresh", () => {
  const accounts: AccountWithUsage[] = [
    {
      ...makeAccount("alpha"),
      usage: makeUsage("alpha", 25),
      usageLoading: true,
      usageUpdatedAt: "2026-03-25T00:10:00.000Z",
    },
  ];
  const nextUsage = makeUsage("alpha", 55);

  const updated = applyUsageFetchResult(
    accounts,
    "alpha",
    nextUsage,
    "2026-03-25T00:30:00.000Z"
  );

  assert.equal(updated[0].usageLoading, false);
  assert.equal(updated[0].usage?.primary_used_percent, 55);
  assert.equal(updated[0].usageUpdatedAt, "2026-03-25T00:30:00.000Z");
});

test("applyUsageFetchResult keeps the previous success timestamp for error payloads", () => {
  const accounts: AccountWithUsage[] = [
    {
      ...makeAccount("alpha"),
      usage: makeUsage("alpha", 25),
      usageLoading: true,
      usageUpdatedAt: "2026-03-25T00:10:00.000Z",
    },
  ];
  const errorUsage: UsageInfo = {
    ...makeUsage("alpha", 25),
    error: "API error: 500",
  };

  const updated = applyUsageFetchResult(
    accounts,
    "alpha",
    errorUsage,
    "2026-03-25T00:30:00.000Z"
  );

  assert.equal(updated[0].usage?.error, "API error: 500");
  assert.equal(updated[0].usageUpdatedAt, "2026-03-25T00:10:00.000Z");
});

test("applyUsageFetchError clears loading without dropping the previous timestamp", () => {
  const accounts: AccountWithUsage[] = [
    {
      ...makeAccount("alpha"),
      usage: makeUsage("alpha", 25),
      usageLoading: true,
      usageUpdatedAt: "2026-03-25T00:10:00.000Z",
    },
  ];
  const errorUsage: UsageInfo = {
    ...makeUsage("alpha", 25),
    error: "network failed",
  };

  const updated = applyUsageFetchError(accounts, "alpha", errorUsage);

  assert.equal(updated[0].usageLoading, false);
  assert.equal(updated[0].usage?.error, "network failed");
  assert.equal(updated[0].usageUpdatedAt, "2026-03-25T00:10:00.000Z");
});
