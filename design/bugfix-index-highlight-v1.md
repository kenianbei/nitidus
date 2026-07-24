# bugfix - Index Highlight - v1

The selected index row renders without its background highlight.

## 1. Symptoms and Causes

**Symptom**: no visible selected-row highlighting in the message index
(reported live against real mail; 1b.8's pty verification couldn't see
it because the plain-text screen replay drops styling).

**Reproduction**: run nitidus against any populated maildir; the selected
row looks identical to unselected rows. Ground truth via pty capture +
terminal-emulator cell inspection: the selected row's cells carry
`bold=true` (the unseen-bold modifier) but the *normal* background
`0f172a` instead of the lightened selected background.

**Root cause**: draw order. The shell still spawns `ContentPane` — the
placeholder themed `Block` that owned the content region from 1a.2
(app shell) — and it renders **after** the index widget spawned by
`IndexPlugin` (shell entities spawn first, but its block draws over the
index output). ratatui's `set_style` overrides fg/bg while OR-ing
modifiers, which is exactly why bold survived and the highlight died.

## 2. Proposed Fix

Delete the placeholder: remove `ContentPane`, its spawn, its
`refresh_content` restyle system, and its startup coalesce effect from
`shell.rs`. The index paragraph pads every row to full width and styles
the whole rect, so the background block is redundant, and the index
widget already carries its own copy of the startup effect. No other
behavior is touched.

## 3. Discussion

### 3.1 R1 (from chat)

Diagnosis and fix were discussed in chat during the live smoke test.
The user asked which list widget the index uses and whether
`tui-widget-list` would help; assessment: the index is hand-rolled
`Line`s in a `Paragraph` over a pre-built window, the crate's
scroll/selection state doesn't cover the domain-specific identity-based
selection, and no list widget would have prevented a draw-order bug.
Staying hand-rolled. User approved the ContentPane removal ("yes").

## 4. Plan

1. `shell.rs`: remove `ContentPane`, `refresh_content`,
   `apply_startup_fx`; update the chrome-spawn test to the two remaining
   widgets.
2. Regression proof is the pty cell inspection (styling is invisible to
   plain-text replay, and the draw-order interaction spans the whole
   render pipeline, which unit tests don't exercise): before the fix the
   selected row's bg equals the normal bg; after, it must differ.
3. `cargo clippy --workspace`, `CARGO_INCREMENTAL=0 cargo test
   --workspace`, pty run against the real Gmail maildir.

## 5. Verification

- Regression red→green via pty cell inspection against the real Gmail
  maildir (100×24, `j` pressed to move the cursor to row 2):
  - Before (1b.8 capture): selected row bg `0f172a` — identical to
    unselected rows, only the unseen-bold surviving.
  - After: selected row bg `696e7a` (the lightened selected
    background), unselected rows unchanged at `0f172a`; statusline
    `2/3` agrees with the highlighted row.
- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **128 passed, 0
  failed** (same count as 1b.8: one chrome test replaced, one widget
  assertion strengthened).

## 6. Implementation Report

Exactly the proposed deletion: `ContentPane`, `refresh_content`, and
`apply_startup_fx` removed from `shell.rs` (the index widget already
carries its own coalesce effect, so startup visuals are unchanged). The
chrome test now asserts the shell owns exactly two widgets — a guard
against any future shell widget silently drawing over the active
screen's content region. The shell module doc states the contract: the
content region belongs to the active screen.

No follow-ups; tab/screen infrastructure (when more screens exist) will
own content-region widget lifecycles per screen.

## 7. Testing and Cleanup
