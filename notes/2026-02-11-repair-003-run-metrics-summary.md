# 2026-02-11 REPAIR-003 repair metrics in run summary

- Added a `repair_failures_total` counter alongside attempts/successes.
- Emitting `repair_attempts_total`, `repair_success_total`, and `repair_failures_total` metric events at the parser boundary.
- `mbus run` now includes per-run repair counts in the final summary log via a telemetry snapshot delta.
