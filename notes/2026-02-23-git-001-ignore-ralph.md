# 2026-02-23 GIT-001 ignore .ralph

## Summary

- `.ralph/` contains local run artifacts/logs and should not be versioned.
- Use `git rm -r --cached .ralph` to untrack existing entries while keeping local files.
- Keep `.ralph/` in repo `.gitignore` to prevent re-tracking.

## Impact

- Prevents noisy diffs and large accidental commits from local agent run outputs.
