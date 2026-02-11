# 2026-02-10 challenge gap assessment

## Context
- User asked whether the repo can already test and satisfy the external 30-step browser challenge (solve all 30 in under 5 minutes, browser-based, with reproducible run stats including time, token usage, token cost).

## What exists now
- `cargo run -- bench` executes a local deterministic harness with 10 tasks from `harness/tasks/*.json`.
- Bench uses `llm_mode=scripted` and pre-baked action sequences, so it does not test autonomous LLM decision-making.
- Bench currently reports pass/fail, steps, duration per task, and summary stats to `target/bench/report.json`.
- `cargo test` unit/integration coverage is strong for core modules; e2e browser test currently fails in this sandbox due to local port bind permission.

## Gaps vs external challenge
- No 30-task target-site harness for the external challenge page.
- No enforcement/reporting of "all 30 solved in under 5 minutes" at run level.
- No token usage accounting in telemetry or run reports.
- No token cost accounting in telemetry or run reports.
- No packaging flow that emits the requested reproducibility bundle (zip + run instructions + run stats artifact).

## Useful next implementation slice
1. Add a challenge runner that drives the real challenge site with `openai` mode.
2. Capture OpenAI usage fields (`prompt_tokens`, `completion_tokens`, `total_tokens`) per call and aggregate.
3. Add configurable pricing and compute USD cost totals in report.
4. Add challenge gate: `solved_count == 30 && total_duration <= 300000 ms`.
5. Add `mbus package` (or script) to output report + instructions and zip artifacts.
