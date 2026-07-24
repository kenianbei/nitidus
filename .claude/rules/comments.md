# Commenting

Since this codebase is written by a single developer who intimately understands
the codebase, always lean toward not adding comments. The code should speak for
itself.

Clear code first; comments second. Naming, types, and small functions carry the
_what_ — see `rules/code.md`. A comment exists to say the thing the code
**cannot** say about itself: an invariant, a constraint, a reason, a limit, a
pointer to the design that justifies it.

## General Guidelines

- Never add comments that narrate the reason for adding the code.
- Never add comments that just document the implementation or coding process you
  used to write the code.
- Never add comments that just repeat what the code says.
- Never add comments that reiterate what is already in the specifications.
- If a comment requires more than two lines, consider moving it into
  documentation.

## Verifying a comment pass

A comment-only change must be exactly that. Strip comments from the before and
after and confirm the files are token-identical — if the code moved, it was not
a comment pass, and it needs the full test treatment.

Then, per `rules/testing.md`:

```bash
cargo clippy --workspace
CARGO_INCREMENTAL=0 cargo test --workspace
```

Confirm the tests actually ran (pass counts present, not merely an absence of
`FAILED`).
