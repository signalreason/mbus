# 2026-02-11 Router failure streak counter

- Updated router to treat failure streaks as consecutive failures only.
- On a NoProgress outcome, failure streak resets while no-progress increments.
- Tier selection still uses the highest tier across failure and no-progress counters.
- Adjusted router unit test expectations to match the new streak semantics.
