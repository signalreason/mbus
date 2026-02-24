# 2026-02-24 ROUTER-006 reset escalation state on progress

- Resetting on confirmed progress now resets ladder index, effort, and trigger counters back to baseline without clearing transition history.
- Added unit coverage for progress reset after model-only tier escalation and effort-only ladder escalation.
