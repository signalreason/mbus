# AGENTS.md

- never do anything twice. script it. build  a tool if you have to. automate.
- remember! when you learn something, write it down. keep notes in whatever
  files you like under `notes/`. i recommend you keep an index of your notes for
  quick reading. you want to only load the notes you need when you need them so
  you don't overload your context and get confused.
- lean on git. commit early and often. use branches based on `main` to do work,
  and use worktrees for most tasks. other agents and humans may be active in
  this repo so make sure your work doesn't impact them.
- use quality commit messages following this spec:
    ```json
    {
      "id": "cbea.git-commit.compact.v1",
      "message_format": "<subject>\n\n<body?>",
      "subject": {
        "single_line": true,
        "max_chars": 50,
        "capitalize_first_char": true,
        "no_trailing_period": true,
        "mood": "imperative",
        "imperative_test_prefix": "If applied, this commit will "
      },
      "body": {
        "present_requires_blank_line_after_subject": true,
        "wrap_hard_at": 72,
        "focus": ["what", "why"],
        "deprioritize": ["how"]
      }
    }
    ```
- use `gh`. when you think a branch is ready to merge, push it to github and
  open a PR, and assign `signalreason` to the PR (don't request review, it won't
  work because you'll be signed in as the user that owns the repo).
- at any time, if the work we are discussing or doing could be more efficiently
  handled with an oss tool, say so before continuing to do work, and only
  continue if i say to do so.
- if anything i've said is confusing or contradictory, stop and ask for
  clarification.
