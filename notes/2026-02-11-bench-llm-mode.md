# 2026-02-11 Bench LLM mode selection

- `mbus bench` now takes `--llm-mode` or config `[llm].mode` and supports `scripted` or `openai`.
- Benchmark runs reject `stub` mode so the harness always uses real scripted actions or OpenAI.
- Scripted benchmark runs generate per-task `llm.actions_file`; OpenAI runs clear it.
