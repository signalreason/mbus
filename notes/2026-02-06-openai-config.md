# OpenAI config notes

- `~/.mbus.toml` can switch to OpenAI by setting `[llm].mode = "openai"` and a non-empty `api_key`.
- `base_url` defaults to `https://api.openai.com/v1`, so it can usually be left as-is.
- Env overrides are available (e.g., `MBUS_LLM_API_KEY`) if you want to avoid storing keys in config files.
- OpenAI chat completions rejects `max_tokens` for some models (e.g., gpt-5*); mbus now sends `max_completion_tokens` using the configured `max_tokens` value.
- If you build `--release`, run `./target/release/mbus` (not `./target/debug/mbus`), or rebuild the debug binary.
