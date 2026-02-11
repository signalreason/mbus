# 2026-02-11 M1-AGENT-001 deterministic max-step loop check

- Verified `AgentLoop::run` already terminates on `Action::Done` or when
  `policy.max_steps` is reached.
- `RunResult.status` explicitly records `Done` vs `MaxSteps` termination.
- Unit tests cover Done termination, max-step exit, and validation short-circuit.
