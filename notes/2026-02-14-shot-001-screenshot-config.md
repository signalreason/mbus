# 2026-02-14 SHOT-001 screenshot config policy

- Added `screenshot` config with `enabled` and `persist` fields (defaults: `enabled=false`, `persist=none`).
- Persist modes: `none`, `on_error`, `always` (case-insensitive; accepts `off`/`disabled` and `on-error` aliases).
- Config precedence remains defaults -> file -> env -> CLI.
- Overrides:
  - File: `[screenshot] enabled = true` / `persist = "on_error"`
  - Env: `MBUS_SCREENSHOT_ENABLED`, `MBUS_SCREENSHOT_PERSIST`
  - CLI: `--screenshot-enabled`, `--screenshot-persist`
