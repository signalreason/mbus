 # 2026-02-24 SHOT-005 screenshot fallback diagnostics

- Screenshot capture failures no longer abort runs; snapshots proceed with text/element observation.
- StepResult now carries `diagnostics` entries for per-step screenshot capture failures.
- Screenshot persistence failures are reported as run summary errors with code `screenshot_persist_failed`.
- Capture error codes: `screenshot_failed` for capture errors and `screenshot_timeout` for capture timeouts.
