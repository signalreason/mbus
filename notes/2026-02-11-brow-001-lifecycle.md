# 2026-02-11 BROW-001 browser lifecycle

- Added a CDP connect option via `CdpConfig.cdp_url` to reuse an existing Chromium instance (http or websocket URL).
- Introduced a `CdpSession` initializer that cleanly separates launch/connect from per-step actions.
- Shutdown now closes the page, closes the browser only when we launched it, and waits briefly for the handler task to finish before aborting.
