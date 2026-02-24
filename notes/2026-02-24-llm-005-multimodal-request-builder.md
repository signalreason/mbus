# 2026-02-24 LLM-005 multimodal request builder

## Summary
- Centralized LLM request assembly in `src/llm/request.rs` with a canonical `LlmRequest` model (text + optional screenshot image part).
- OpenAI adapter now converts the canonical request into chat-completions payloads, emitting image parts as `image_url` data URLs when screenshot bytes are available.
- Added golden test fixture for multimodal payload shape and kept prompt construction logic shared across providers.
