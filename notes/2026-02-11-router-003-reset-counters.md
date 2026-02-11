# 2026-02-11 ROUTER-003 reset counters on progress

- Moved progress predicate (`state_hash` change) into `llm::router::step_outcome` so loop and router share it.
- Router counters reset when progress is detected via the shared predicate, with a mixed-outcome unit test.
