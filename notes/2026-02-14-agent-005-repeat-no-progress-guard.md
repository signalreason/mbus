# 2026-02-14 AGENT-005 repeat no-progress action guard

- Added a loop-level validation guard in `src/agent/loop.rs` that rejects an action when:
  - the current `state_hash` matches recent step hashes, and
  - the exact same action already produced `NoProgress` or `ValidationFailed` in that unchanged state.
- New validation code: `repeat_no_progress_action`.
- Added prompt-side mitigation in `src/llm/openai.rs`:
  - includes `StateHashStreak` in the user prompt,
  - includes `RecentHistoryTail`,
  - includes explicit execution rules to avoid repeating no-op actions and to respect scroll/wait bounds.
- Added tests:
  - `rejects_repeated_action_after_no_progress_in_same_state` in `src/agent/loop.rs`,
  - `prompt_includes_state_hash_streak` in `src/llm/openai.rs`.
