# 2026-02-14 PRD M4 visual-state + effort-escalation spec

- Added `Milestone 4: Visual-state grounding + effort escalation` to `prd.md`.
- Expanded scope/requirements to include:
  - per-step screenshot artifact capture in `Observation`
  - multimodal LLM contract (image + structured text)
  - two-axis routing policy across model tier and reasoning effort
  - deterministic fallback when screenshot capture fails
- Added acceptance criteria requiring escalation triggers on unchanged-state streaks and repeated validation codes (for example `repeat_no_progress_action`) with reset on confirmed progress.
- Added engineer-ready tasks `18` to `22` in `Milestone M4`:
  - screenshot artifact channel
  - multimodal prompt contract
  - two-axis escalation policy
  - regression suite for unchanged-state/repeat-validation loops
  - optional visual diff + OCR evaluator spike
