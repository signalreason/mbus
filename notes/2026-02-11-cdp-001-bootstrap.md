# 2026-02-11 CDP-001 Bootstrap Session

- Added a minimal CDP bootstrap path that launches headless Chromium and tears it down.
- `CdpBrowser::launch` now maps launch/page/navigation failures to explicit error codes:
  - `cdp_launch_failed`
  - `cdp_page_failed`
  - `cdp_nav_failed`
- Added a small binary for manual validation: `cargo run --bin cdp_bootstrap`.
