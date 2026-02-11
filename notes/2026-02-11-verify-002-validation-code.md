# 2026-02-11 VERIFY-002 validation codes

- Added `StepError.validation_code` to carry the first validator error code for machine-readable error handling.
- Validation failures still use `code = "invalid_action"`, with `validation_code` set to the specific rule (e.g., `unknown_id`).
