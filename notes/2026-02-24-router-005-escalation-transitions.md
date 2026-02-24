# 2026-02-24 ROUTER-005 escalation transitions with reason codes

- Added ladder transition policy keyed to unchanged-state streaks and repeated validation codes.
- Router now records structured transition reasons plus resulting ladder step (model/tier/effort).
- Agent loop applies the ladder policy after validation failures and step outcomes, emitting a `router_transition` trace event.
