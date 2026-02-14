# 2026-02-14 AGENT-006 repeat-validation recovery hardening

- Added LLM context wiring for recent step outcomes:
  - Extended `LlmClient::propose_action` signature to include `steps: &[StepRecord]`.
  - `OpenAiClient` prompt now includes `RecentStepFeedback` with compact fields:
    - `action`, `outcome`, `result_ok`, `error_code`, `validation_code`, `validation_codes`, `new_state_hash`.
  - Prompt execution rules now explicitly forbid re-proposing actions previously blocked with `validation_code=repeat_no_progress_action`.
- Added loop-level breaker in `AgentLoop`:
  - Tracks `repeat_no_progress_action` validation failures in the current unchanged state hash.
  - Terminates early as `RunStatus::NoProgress` after 3 repeated blocked-action validations.
  - Emits `repeat_no_progress_termination` trace event.
- Tests:
  - Added `prompt_includes_recent_step_feedback` in `src/llm/openai.rs`.
  - Added `terminates_when_repeat_no_progress_validation_loops` in `src/agent/loop.rs`.
  - Updated all LLM test doubles and integration test mocks for new trait signature.
