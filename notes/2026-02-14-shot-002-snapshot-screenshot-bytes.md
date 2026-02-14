# 2026-02-14 SHOT-002 snapshot screenshot bytes

- Added viewport screenshot capture at the CDP snapshot boundary when `screenshot_enabled` is true.
- Stored last screenshot bytes on `CdpBrowser` for test visibility via `take_last_screenshot()`.
- Wired app config screenshot enable flag into `CdpConfig`.
- Added e2e harness test that asserts non-empty screenshot bytes are captured.
