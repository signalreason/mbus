# 2026-02-11 HARNESS-003 select/scroll/wait integration coverage

- Added dedicated integration tests for select, scroll, and wait actions.
- Each test asserts success behavior plus bounded-constraint failures (invalid select option, scroll out of bounds, wait too long).
- Harness page now includes a tall spacer to ensure scroll actions move the viewport.
