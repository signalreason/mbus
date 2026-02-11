# 2026-02-11 Bench gate checks

- Added a shared bench gate evaluation function so both scripted and OpenAI benchmark modes use the same pass/fail logic.
- Bench report now includes an explicit `gate` object with pass/fail status and a failure reason when the gate fails.
- Step-limit enforcement remains centralized in `evaluate_task`; tests assert the step limit failure path.
