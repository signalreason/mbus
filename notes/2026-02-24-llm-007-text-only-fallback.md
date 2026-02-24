# 2026-02-24 LLM-007 text-only payload fallback

- Added explicit `payload_mode` to LLM request payloads (`text_only` vs `multimodal`).
- Request builder sets `multimodal` only when both screenshot metadata and bytes are present; otherwise falls back to text-only.
- Added tests to cover text-only fallback and ensure prompt still includes core observation fields.
