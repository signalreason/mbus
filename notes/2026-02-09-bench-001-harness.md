# 2026-02-09 BENCH-001 benchmark harness

- Added `mbus bench` to run a local deterministic harness from `harness/tasks/*.json`.
- Bench fixtures contain: task text, start path, scripted actions, and assertions for final status/url/text.
- The harness spins up a local HTTP server (`127.0.0.1:0`) and injects `{{base_url}}` into fixture `navigate` URLs.
- Bench writes `target/bench/report.json` with per-task results and summary metrics (`completion_rate`, `median_steps_success`, `p95_steps_success`, gate result).
- Default gate is `required_passes = total_tasks - 2` (for 10 tasks, 8 required), with `max_steps_per_task` default 40.
- In restricted sandboxes, local socket bind may require elevated permissions for `mbus bench`.
