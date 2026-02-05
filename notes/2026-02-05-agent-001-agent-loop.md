# 2026-02-05 AGENT-001 agent loop

- Implemented `agent::r#loop::AgentLoop` with snapshot -> propose -> validate ->
  apply -> snapshot cycle and max-step termination.
- Added `llm::client::LlmClient` trait + `LlmError` and wired tier selection
  through `llm::router::Router`.
- Added `agent::memory` to store plan, bounded observations, and step history.
- Validation failures produce `StepResult` with `invalid_action` and count as
  failure; `Done` short-circuits without browser apply.
- Progress is computed from state-hash changes between snapshots; failures do
  not attempt no-progress classification.
- Unit tests cover Done termination, validation skip apply, and max-steps exit
  with scripted LLM + fake browser.
