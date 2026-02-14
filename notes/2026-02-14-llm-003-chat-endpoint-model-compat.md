# 2026-02-14 LLM-003 chat endpoint model compatibility

## Summary
- `src/llm/openai.rs` posts to `/v1/chat/completions`.
- `*-codex` models (for example `gpt-5.2-codex`, `gpt-5.1-codex-max`) fail on that endpoint with HTTP 404 and `not a chat model`.
- Challenge runs should use chat-compatible models when the adapter stays on chat completions.
- GPT-5 chat models can reject non-default `temperature` values; `temperature=0.2` returned `unsupported_value` for `param=temperature`.

## Verified behavior
- Fails on chat completions: `gpt-5.2-codex`, `gpt-5.1-codex-max`, `gpt-5.1-codex`, `gpt-5.1-codex-mini`.
- Works on chat completions: `gpt-5-mini`, `gpt-5.1`, `gpt-5.2`, `gpt-4.1`, `gpt-4o`.

## Action taken
- Updated defaults/examples to chat-compatible models:
  - fast: `gpt-5-mini`
  - mid: `gpt-5.1`
  - strong: `gpt-5.2`
- Updated default/example `temperature` to `1.0`.
- Added chat-request retry behavior that removes `temperature` and retries once when OpenAI returns `unsupported_value` on `temperature`.

## Follow-up
- If codex-specific models are required, add a Responses API adapter path and route those model ids there instead of chat completions.
