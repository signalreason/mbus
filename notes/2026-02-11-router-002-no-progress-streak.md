# 2026-02-11 M1-ROUTER-002 no-progress streak counter

- Changed `step_outcome` to mark `NoProgress` only when the state hash is unchanged.
- Kept progress heuristics logging (actionables, low actionability) but stopped using them for no-progress escalation.
- Updated agent loop tests to align with hash-only no-progress detection.
- Rationale: PRD requires unchanged state hash to increment no-progress counter; hash comparison at loop boundary keeps deterministic escalation.
