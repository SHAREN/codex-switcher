import assert from "node:assert/strict";
import test from "node:test";

import { formatResetTime } from "../src/components/UsageBar";

test("formatResetTime includes days for long reset windows", () => {
  const nowSeconds = 1_700_000_000;
  const resetAt = nowSeconds + (((4 * 24 + 22) * 60 + 6) * 60);

  assert.equal(formatResetTime(resetAt, nowSeconds), "4d 22h 6m");
});

test("formatResetTime keeps hour and minute format under one day", () => {
  const nowSeconds = 1_700_000_000;
  const resetAt = nowSeconds + ((22 * 60 + 6) * 60);

  assert.equal(formatResetTime(resetAt, nowSeconds), "22h 6m");
});
