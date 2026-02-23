# 2026-02-23 SHOT-003 screenshot artifacts

- Added screenshot artifact persistence under `.ralph/runs/<run_id>/steps/step-<index>/screenshot.png` with `step://<run_id>/step-<index>/screenshot.png` references.
- Output artifacts now include optional screenshot metadata: MIME type (`image/png`), SHA-256 digest, step index, and byte size.
- Screenshot persistence is gated by config: `always` persists; `on_error` persists when terminal state is not `done`.
