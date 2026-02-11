# 2026-02-11 BROW-007 wait action guard

- Added a wait max guard in the browser action applier so oversized waits return a structured error (`wait_too_long`).
- Threaded the validator max wait value into the browser config to keep execution limits consistent.
- Ensured wait actions get a timeout that can cover the requested sleep duration.
