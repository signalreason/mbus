# 2026-02-11 CORE-002 module boundaries cleanup

- Moved JSON repair from `verify` to `llm` to remove the `llm -> verify -> llm` cycle.
- `llm::repair` now owns `repair_action` and continues to use `ActionSchema` + `Action`.
- `verify` only contains validation rules; `llm` owns LLM-specific parsing/repair utilities.
