# 2026-02-05 EXTRACT-001 extraction output

- Added `ExtractResult` on `StepResult` so extract actions return a value alongside ok/error.
- New output module writes `ExtractOutput` JSON with `task_id`, `timestamp`, and `extracts[]` (step_index, query, id, value).
- Output path configurable via `--extract-output`, config `output.extract_output`, env `MBUS_EXTRACT_OUTPUT`; defaults to `mbus_extract.json`.
- `task_id` is a deterministic hash of the task string.
