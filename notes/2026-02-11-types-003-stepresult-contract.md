# 2026-02-11 TYPES-003 StepResult contract

- StepResult already carries `ok`, structured `StepError`, `new_state_hash`, and optional `extract`.
- Agent loop now stamps `new_state_hash` after taking the post-apply snapshot, so action outcomes record the new state hash when available.
- Validation failures also include the current observation hash to keep StepResult envelopes consistent.
