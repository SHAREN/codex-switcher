# Work Tracker

This file tracks non-bug project work for Codex Switcher.

Canonical active tracker:
- Plane project `CSW` (`Codex Switcher`)

Status values:
- `backlog`: parked idea or future work
- `todo`: agreed and ready, but not started in the current thread
- `in progress`: active work is happening now
- `blocked`: waiting on an external dependency or decision
- `done`: completed and verified
- `cancelled`: intentionally dropped

## Active Items

### TASK-001: Organize repository and move work tracking into Plane
- Type: chore
- Status: in progress
- Tracked since: 2026-03-26
- Goal:
  - Set up repo-local Plane integration for this repository.
  - Import the existing `BUG_TRACKER.md` entries into Plane.
  - Capture the current dirty tree as a managed cleanup task with comments.
  - Mirror each meaningful Git commit with a short update in the related Plane item.
- Current evidence:
  - The initial `git status --short` snapshot showed 20 modified tracked files and 9 new paths/directories before tracker setup changes were added.
  - The main dirty-tree clusters are auth/account storage, usage sync and card ordering, LAN dashboard and process monitoring, tests, and local temp artifacts.
  - The repository already had `BUG_TRACKER.md`, but no repo-local Plane configuration.
- Progress:
  - Created the Plane project `Codex Switcher` (`CSW`) on 2026-03-26.
  - Added `.codex/plane-work-tracker.json` for repeatable repo-local sync.
  - Imported `BUG-001` through `BUG-003` into Plane with direct API fallback because the helper scripts did not match the current markdown heading format and filtered lookup behavior.
  - Added ignore rules for `.tmp/` and `.codex-temp/`.
- Next steps:
  - Group the current uncommitted source changes into logical future commits.
  - Add commit hashes and short summaries to Plane comments as the cleanup continues.
  - Keep new follow-up tasks in Plane instead of losing them inside the dirty tree.

## Workflow Notes

- Plane is the source of truth for active work items. Local markdown files stay in the repo as quick context and migration-friendly source material.
- When a commit hash exists, add it to the related Plane item with a short note about what landed in that commit.
- Local scratch artifacts and one-off logs should live in `.tmp/` or `.codex-temp/` and stay out of Git.
