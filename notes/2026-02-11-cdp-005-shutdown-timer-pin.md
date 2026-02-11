# 2026-02-11 CDP-005 shutdown timer pin

- `tokio::select!` in CDP session shutdown now pins the sleep timer to satisfy `Unpin` requirements.
- This avoids build failures when compiling the shutdown path.
