# 2026-02-11 M2-PROGRESS-001 hash streak

- Added explicit loop state for repeated identical state hash streak.
- Terminate runs when `max_no_progress_steps` is reached; emit `RunStatus::NoProgress` with a Done summary.
- Wire config/CLI/env for `max_no_progress_steps` and log it in config output.
- No-progress streak now feeds router outcomes via `step_outcome` parameter.
- Added agent-loop test for no-progress termination.
