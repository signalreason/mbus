# 2026-02-14 LLM-004 empty-output diagnostics and recovery

## Problem
- OpenAI chat completions can return successful responses with empty usable content (`message.content` blank or non-text parts).
- The agent previously terminated the run on `empty_response` at the loop boundary.

## Changes
- `src/llm/openai.rs` now logs structured diagnostics for empty output cases:
  - `finish_reason` (from choice metadata)
  - content shape (`content_kind`, `content_part_types`, `content_text_chars`)
  - refusal shape/size (`refusal_kind`, `refusal_chars`)
  - token usage (`prompt_tokens`, `completion_tokens`, `total_tokens`)
- Empty-output diagnostics are emitted on both initial attempt and retry via `event="llm_empty_output"`.
- `src/agent/loop.rs` now treats `error_code="empty_response"` as recoverable:
  - logs `event="llm_error_recoverable"`
  - records a router failure and continues, allowing tier escalation instead of immediate run abort.

## Tests
- `llm::openai::parse_tests::collect_empty_output_diagnostics_reads_finish_reason_and_usage`
- `agent::r#loop::tests::empty_response_error_escalates_and_recovers`
- `agent::r#loop::tests::non_recoverable_llm_error_still_terminates`
