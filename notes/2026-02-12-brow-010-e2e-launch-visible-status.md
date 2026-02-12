# 2026-02-12 BROW-010 e2e launch/profile isolation + status text visibility

## Context
- `tests/e2e.rs` started failing with mixed symptoms:
  - browser launch failures with Chromium `ProcessSingleton` lock errors
  - click/type/select assertions not observing status updates in `visible_text`

## Findings
- `chromiumoxide` defaults launch profile path to a shared temp dir (`chromiumoxide-runner`) when `user_data_dir` is not set.
- Parallel e2e tests then race on the same profile lock and some launches abort.
- Observation `visible_text` compaction did not include live region/status nodes, so harness status updates (`clicked`, `typed:*`, `selected:*`) were omitted from snapshots.

## Changes
- In `src/browser/cdp.rs`:
  - assign a unique per-launch `user_data_dir` in temp using `pid + timestamp + counter`
  - track that path on the session and best-effort remove it during shutdown
- In `src/browser/observe.rs`:
  - include visible live-region/status nodes (`[role='status']`, `[role='alert']`, `[role='log']`, `[aria-live]`) in `visible_text`

## Validation
- `cargo test --test e2e -- --nocapture`: 8 passed
- `cargo test`: full suite passed
