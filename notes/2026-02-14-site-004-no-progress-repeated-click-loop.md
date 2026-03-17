# 2026-02-14 Site-004 repeated-click no-progress loop

Historical note. Superseded by `docs/live-eval-policy.md` and the challenge-first status docs. This note captures one exploratory live-site failure pattern.

- Observed run signature: repeated `click` on the same element id with `new_state_hash` unchanged across many steps, ending in `status=no_progress`.
- This indicates action execution succeeded but the selected action is a no-op for current page state.
- In this challenge site, repeated verify/next clicks without the correct step code cannot advance; the model keeps selecting the same control.
- Local contributing factor:
  - `SYSTEM_PROMPT` is minimal and does not explicitly forbid repeating the same action after unchanged state hashes.
  - Prompt context includes `History` actions, but no explicit step outcomes/reasons.
- Immediate mitigation:
  - Fail fast on no-progress (`max_no_progress_steps` lower, e.g. 6-8) to avoid wasting tokens.
  - Add prompt rule: if state hash is unchanged, do not repeat the same element id action; choose a different action.
  - For this site specifically, route via `/` entrypoint and use deterministic navigation goal (e.g. direct `/finish`) or add a JS/session action to read `wo_session` codes.
