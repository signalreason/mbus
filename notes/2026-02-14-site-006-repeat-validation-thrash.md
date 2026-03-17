# 2026-02-14 Site-006 repeat-validation thrash in unchanged state

Historical note. Superseded by `docs/live-eval-policy.md` and the local challenge-first proof path.

- Run signature on `step1?version=3`:
  - First no-op appears as `outcome="no_progress"` with unchanged `new_state_hash`.
  - Then same click id is repeatedly proposed and rejected with `validation_code="repeat_no_progress_action"`.
  - Interleaved scroll attempts keep the same state hash, so the run keeps burning LLM calls until `max_no_progress_steps` termination.
- Root cause:
  - Loop-level guard works (it blocks repeated no-op action), but recovery is weak.
  - LLM prompt has action history but not explicit recent validation/result feedback, so model can keep proposing the blocked id.
- Mitigations:
  - Add recent step outcomes/validation codes to LLM prompt context.
  - Add a loop-level breaker for repeated `repeat_no_progress_action` in the same state hash.
  - Keep `agent.max_no_progress_steps` low (about 6-8) to cap wasted calls in dead states.
