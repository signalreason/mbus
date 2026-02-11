# 2026-02-11 AGENT-003 observation window

- Memory observation window is passed into LLM prompt as `RecentObservations`.
- Order is stable (oldest to newest) via `VecDeque` serialization.
- Window stays bounded by `memory.max_observations`.
