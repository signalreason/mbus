# BROW-008 back action execution

- `Action::Back` now checks `history.length` before navigating.
- When there is no previous entry (`<= 1`), action returns `ActionError` with code `no_history` and a clear message.
- Back still uses `history.back()`; navigation change is expected to be detected by the next snapshot.
