# 2026-02-24 LLM-008 multimodal action parsing regression coverage

- Added OpenAI parse tests that compare error codes for malformed/unknown actions between text-only string content and multimodal content arrays.
- Coverage includes invalid JSON, unknown action types (schema violations), and multi-action arrays to ensure the single-action contract stays unchanged.
