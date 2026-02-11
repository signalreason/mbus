# 2026-02-11 EXTRACT-002 extract artifact schema + append-safe write

- Extract artifacts are now newline-delimited JSON records appended per run, not a single overwritten file.
- Schema is versioned (`schema_version: 1`) and includes `run_id`, `task`, `task_id`, and RFC3339 `timestamp`.
- `run_id` is derived from `task_id` and `timestamp` to keep runs distinct without adding new deps.
