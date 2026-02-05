# CLI-001: CLI + config

- Added `mbus run` CLI with clap, plus config file + env + CLI override precedence.
- Config merge order: defaults -> config file -> env vars (`MBUS_*`) -> CLI flags.
- LLM modes: `stub` (default), `scripted` (actions file), `openai` (chat completions).
- OpenAI client uses `/chat/completions` with system prompt + task/plan/obs/history/schema; response must be pure JSON.
- JSON logs: emits `config`, per-step `step` logs, and final `summary` log with status.
- Env overrides supported for browser, agent, router, validator, and LLM fields (see `src/config.rs`).
