# refactor - UI - v1

**STUB** — parked by agreement during refactor-keymap-v1 (2026-07-25): the
scope below is claimed but undesigned. Pick this up with a full discussion
round before any planning or implementation; sections 3–7 intentionally
empty until then.

A broader interaction-surface refactor for nitidus, collecting the UI-shape
ideas that outgrew keybinding work.

## 1. Current Design

- **The pager is a screen, not a pane**: it replaces the index in the
  content region (`Screen` enum), with explicit open/fetch/close
  semantics. The sidebar is the only side-by-side pane in the mail tab.
- **Feedback is statusline-bound**: y/n confirmations run through the
  bottom-row prompt, and errors/notices land in the statusline's center
  segment (with the toast plugin so far used sparingly). Destructive
  confirms, multi-line errors, and anything that should interrupt visually
  all compete for one line of text.
- Overlay machinery exists (picker panels, the explorer modal, completion
  panels) and the theme system provides styled surfaces — the building
  blocks for richer modals are present but underused.

## 2. Proposal (headline items, to be designed)

1. **Preview as a third pane** (yazi-style miller columns): sidebar |
   index | preview, the preview following the index selection. Known hard
   parts recorded from the R2 discussion that spawned this doc:
   selection-driven auto-fetch needs debounce and stale-fetch
   cancellation; `Screen`/focus semantics need rethinking (is the pager a
   pane focus rather than a screen?); width budgets on narrow terminals
   need collapse rules; the keymap context should follow the focused pane.
   The arrow navigation shipped by refactor-keymap-v1 (`←` out / `→` in)
   is forward-compatible with this layout.
2. **Confirmations as overlays**: y/n questions (permanent delete, discard
   message, delete contact) become centered modal overlays instead of
   statusline prompts — harder to miss, room for context (what exactly is
   being deleted).
3. **Errors and notices via toasts**: route severity-appropriate feedback
   through the toast system (already a dependency) instead of the
   statusline center segment — multi-line capable, stacking, self-expiring
   — keeping the statusline for state, not events.

Out of scope until designed: everything above in detail, plus any
interaction with the phase 2 items (leader menus, which-key hints,
selectable keymap schemes) that a redesigned surface would touch.

## 3. Discussion

## 4. Plan

## 5. Verification

## 6. Implementation Report

## 7. Testing and Cleanup
