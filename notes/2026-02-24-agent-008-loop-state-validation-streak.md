# 2026-02-24 AGENT-008 loop state validation streak tracking

- Extended loop state to track repeated validation code streaks alongside state-hash streaks.
- Validation streak increments only when the same validation code repeats in the same state hash.
- Streak resets on successful validation (apply or done) and when validation code or state hash changes.
