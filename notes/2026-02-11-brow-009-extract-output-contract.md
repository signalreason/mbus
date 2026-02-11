# 2026-02-11 BROW-009 extract output contract

- Extraction payload is standardized via `ExtractResult` on `StepResult` (query, id, value).
- `id` is now always serialized (null when absent) to keep extract payload schema stable across runs.
- Extract output artifacts mirror the same `id` presence for deterministic downstream parsing.
