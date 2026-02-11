# 2026-02-11 PRD M3 autonomous bench spec

- Added `Milestone 3: Autonomous benchmark proof` to `prd.md`.
- New milestone objective: validate harness success with autonomous LLM decisions (`openai` mode), not only scripted playback.
- Added acceptance criteria requiring:
  - bench mode selection (`scripted` and `openai`)
  - token usage reporting (`prompt_tokens`, `completion_tokens`, `total_tokens`)
  - configurable token pricing and aggregate USD cost
  - existing gate enforcement preserved
  - reproducible run artifact fields (mode, models, timing, gate, usage/cost totals)
- Added engineer-ready task `17. [Build] Autonomous benchmark mode + cost telemetry` in `Milestone M3`.
