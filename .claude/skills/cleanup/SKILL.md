---
name: cleanup
description:
  Prune redundant comments and dead code from a bounded scope, then prove the
  build still passes.
---

# Cleanup: comment & dead-code hygiene

Removes comments that no longer earn their place and code that nothing uses,
then verifies the result. Follows `rules/comments.md` (clear code over comments)
and `rules/code.md` (the hard limits).

## Scope it first — never sweep the whole tree

An unbounded "look through all files" over the workspace is not actionable.
Default to the **current branch's diff** (`git diff --name-only main...HEAD`)
unless the user names a file, module, or crate. State the scope you chose before
touching anything.

## Comment pass

For each comment in scope, decide:

1. **Irrelevant** — the code already says it, or it describes something that no
   longer exists. Remove it. No approval needed.
2. **Redundant** — it repeats a nearby comment, a doc comment, or a design doc.
   Remove it. No approval needed.
3. **Unclear** — right to exist, wrong wording. Reword so both a human and the
   next agent grasp it in one read. No approval needed.
4. **Belongs in docs** — it explains a decision better kept in a `design/*.md`
   or a doc comment. Propose the destination and the rewrite, and ask the user
   to confirm placement.

List every comment you removed or reworded in the final summary so the change is
auditable — silent deletion hides mistakes.

## Dead-code pass

Do not guess whether code is dead — prove it with tools:

- `cargo build --workspace` and read the `dead_code` / `unused` warnings.
- `cargo clippy --workspace` for unreachable and never-read items.
- Grep for callers before removing anything the compiler didn't already flag (a
  `pub` item may be used across crates or by tests).

Then:

1. **Provably unused** (compiler-flagged, no callers) — remove it, but confirm
   with the user first; a `pub` API may be an intentional seam.
2. **Poorly written or misplaced** — propose a refactor and get approval before
   starting. If it cascades across more than 3 files, write a short plan first
   (per `rules/code.md`).

## Verify before you finish

Cleanup that breaks the build is worse than no cleanup. Run, in order:

```bash
cargo fmt --all
cargo clippy --workspace
CARGO_INCREMENTAL=0 cargo test --workspace
```

Confirm the tests actually ran (pass counts present, not just "no FAILED")
before reporting done.
