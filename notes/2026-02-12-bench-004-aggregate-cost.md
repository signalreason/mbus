# 2026-02-12 BENCH-004 aggregate usage + cost

- Added benchmark aggregation module for per-step usage, per-task totals, and aggregate totals.
- Added configurable per-1M pricing (input/output) in LLM config via file/env/bench CLI overrides.
- Bench report now includes aggregate token usage and estimated USD cost with pricing metadata.
- Unit tests cover cost math and pricing-required behavior.
