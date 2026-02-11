# OBS-003 compact visible_text policy

- Implemented a compact `visible_text` extractor that prioritizes visible interactive elements, nearby container text, and headings instead of full-page dumps.
- JS collector avoids sensitive form values by default by skipping text nodes under `input`, `textarea`, `select`, `option`, and `contenteditable` elements.
- Output is de-duplicated, per-chunk length capped, and overall trimmed in Rust via the existing `max_text_len` truncation.
- Fallback uses filtered body text only when no other chunks are found.
