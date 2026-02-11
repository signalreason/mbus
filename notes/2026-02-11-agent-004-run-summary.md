# 2026-02-11 AGENT-004 run summary

- Added a run summary builder that derives counts and per-step errors from `StepRecord` history.
- Summary now includes `terminal_state`, `steps`, `errors`, and `output_artifacts`, plus step count breakdowns.
- CLI emits the summary even when the run fails (agent error, startup error, or output write error).
- Extract output writes now return an artifact entry with path and record count.
