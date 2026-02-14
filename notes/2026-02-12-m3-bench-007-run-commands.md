# 2026-02-12 M3-BENCH-007 run commands

- `mbus bench` supports `--llm-mode openai` and writes a JSON report with gate, duration, aggregate usage, and aggregate cost fields.
- Cost totals require pricing to be set via `--llm-input-cost-per-million` and `--llm-output-cost-per-million` (or config/env); otherwise cost fields are null with `error="missing_pricing"`.
- For reproducible artifacts, write report to a timestamped path that includes model identifiers, then archive it (or check it in) and record key fields with `jq`.
