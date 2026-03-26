# Bug Tracker

This file tracks user-visible bugs and transport issues for this project.

Plane note:
- Active work items now also live in Plane project `CSW`.
- Keep this file as repo-local bug history and migration-friendly source text.

Status values:
- `open`: reproduced and not yet investigated
- `investigating`: evidence collected, root cause not fully closed
- `fixed`: fix landed and verified
- `partially fixed`: one path fixed, residual risk remains

## Active Bugs

### BUG-001 LAN dashboard unreachable from other devices
- Status: partially fixed
- First confirmed: 2026-03-22
- Symptom: `http://192.168.0.119:3210` does not open from another device even though the panel starts locally.
- Reproduction clues: start the LAN dashboard, then try to open the host IP and port from another device on the same network.
- Current evidence:
  - `codex-web.exe` listens on `0.0.0.0:3210`.
  - Local requests to `127.0.0.1:3210` and `192.168.0.119:3210` succeed on the host PC.
  - Windows Firewall ActiveStore contains inbound `Block` rules for `codex-web.exe`.
  - Windows Firewall ActiveStore contains inbound `Allow` rules for `node.exe`.
- Fix/workaround:
  - `pnpm lan` now starts a Node LAN proxy on `0.0.0.0:3210`.
  - The Rust backend is moved behind it on `127.0.0.1:3211`.
  - LAN traffic now enters through `node.exe`, which already has inbound allow rules.
- Root cause / hypothesis: inbound traffic is blocked by Windows Firewall for the Rust web binary, so the process is reachable only from the host itself.
- Verification:
  - Local verification passed after the switch: `:3210` is owned by `node.exe` and `GET /api/health` returns `{"ok":true}` on both `127.0.0.1:3210` and `192.168.0.119:3210`.
  - Cross-device verification is still needed from a second device on the LAN.

### BUG-002 Other Accounts reorder during sync
- Status: fixed
- First confirmed: 2026-03-22
- Symptom: `Other Accounts` cards jump around while usage is syncing, especially after pressing `Sync`.
- Reproduction clues: open the app, keep the default `Reset: earliest to latest` sort, then trigger a usage sync on one or more accounts.
- Current evidence:
  - `src/App.tsx` used an effect that depended on the `otherAccounts` array identity, so every per-account usage update retriggered sorting during a single sync cycle.
  - `refreshUsage()` updates accounts independently as requests resolve, which exposed those repeated re-sorts.
  - `primary_resets_at` from the backend changes at second precision, while the UI displays reset times only at minute precision, so cards could reorder even when the visible labels still looked the same.
  - `src-tauri/src/api/usage.rs` also returned `refresh_all_usage()` results in completion order, not input order.
- Root cause / hypothesis: the UI allowed sync-driven state updates to reapply ordering, so both manual refreshes and background sync cycles could reshuffle `Other Accounts` even when the user had not changed the selected sort mode.
- Fix/workaround:
  - Apply the chosen sort when the user changes sort mode or when the list is first initialized.
  - Preserve the current `Other Accounts` order across sync and usage refresh cycles, only repairing membership when accounts are added, removed, renamed, or switched.
  - Compare reset times at minute precision when a sort is applied so the sort matches what the UI actually renders.
  - Preserve input order in the backend `refresh_all_usage()` helper as well.
- Verification:
  - `pnpm build`
  - `cargo check --manifest-path src-tauri/Cargo.toml`
  - `pnpm test:sync-order`
  - `pnpm test:sync-order:ui`

### BUG-003 Codex App status shows stale approval and raw timestamps
- Status: fixed
- First confirmed: 2026-03-22
- Symptom: the `Codex App` panel can stay on `Approval needed` after the approval was already confirmed, shows raw ISO timestamps like `2026-03-22T10:35:46.727Z`, and duplicates process/helper badges in the panel.
- Reproduction clues: open the LAN dashboard while Codex Desktop has a recent approval flow or completed turn, then wait for the activity polling to refresh.
- Current evidence:
  - Desktop logs contain a later `Sending server response ... method=item/commandExecution/requestApproval ...` marker after the original approval notification.
  - Desktop logs also contain `[desktop-notifications] show turn-complete` markers for finished turns.
  - The backend activity parser previously ignored both markers, so `Approval needed` could remain latched on an old `show approval` event.
  - The frontend rendered `last_event_at` only through backend summary text, which left raw ISO timestamps visible in the UI.
- Root cause / hypothesis: the activity heuristic modeled approval as a one-way state transition and did not parse approval resolution or turn completion notifications from the desktop logs; the panel layout also kept process/helper diagnostics in the primary status area instead of the header.
- Fix/workaround:
  - Parse approval resolution responses and `turn-complete` notifications from desktop logs.
  - Treat resolved approvals as `busy` until completion, and completed turns as `idle`.
  - Render `last_event_at` in a human-readable local format in the frontend.
  - Move the process-count badge to the header and remove the `helpers ignored` chip from the panel.
- Verification:
  - `cargo test --manifest-path src-tauri/Cargo.toml process`
  - `pnpm build`
  - Live `POST /api/invoke/get_codex_activity` now returns a fresh `busy` state for the newer conversation instead of the stale approval state.
  - Playwright verification on `http://127.0.0.1:3210/` shows `1 Codex app instance running` in the header, `AI busy` in the panel, and a human-readable timestamp (`Since 22 мар., 16:08.`).

## Fixed Bugs

None yet.

Workflow note: update this file in the same task when a bug is investigated or fixed, and mirror meaningful progress in the related Plane item.
