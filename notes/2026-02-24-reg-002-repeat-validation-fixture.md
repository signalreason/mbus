# 2026-02-24 REG-002 repeated validation escalation fixture

- Added an end-to-end fixture that repeats a `Wait` action on the static loop page so the
  `repeat_no_progress_action` validator triggers in an unchanged state.
- Test asserts repeated validation failures, unchanged `state_hash`, and a router ladder
  transition with reason `repeat_validation_code` (streak=2) to confirm escalation logic.
- Fixture uses the public agent loop and validator path via `CdpBrowser` to mirror production.
- Avoided consuming ladder transitions on validation failures via unchanged-state triggers so
  `repeat_validation_code` transitions are observable when the validation streak advances.
