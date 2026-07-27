# feature - Index Rows - v1

Turn the message index from a one-line-per-message table into an Outlook-style
three-line card — sender, subject, date — and rebalance the pane budget so the
list column is a fixed width and the reading pane takes the slack.

(A body-preview line was proposed and dropped in R1; see §2 and §3.2.)

## 1. Current Design

### The row is a line, and everything counts in lines

`index/render.rs` builds one `IndexRow` per message and `row_line` renders it as
exactly one `ratatui::Line`: cells fitted to built-in widths — flags(4),
date(12), from(30% of the pane, max 30), subject(the remainder) — joined by a
single-space gap and padded to the pane width. Column order and subset come from
`[ui.index] columns` (`feature-index-custom-v1`); `subject` absorbs the slack.

Because a row _is_ a line, every piece of row arithmetic in the crate equates
the two:

- `index/mod.rs::refresh_index` takes the viewport as `last_height` (the widget
  rect's height, in terminal lines) and hands it to `scrolled_top` and the
  window builder as a count of _rows_.
- `render_index` takes `area.height` rows off the window and renders them as a
  `Paragraph` of lines.
- `index/mouse.rs::absolute_row` maps a click to `window_top + local_row`.
- `ops` page motions (`Motion::NextPage`) use the same height as a page size.
- `filter::match_range` highlights the first match in the one fitted line, so
  search highlighting is inherently per-line.

### The pane budget

`panes.rs` declares three columns: folders (`Fixed(SIDEBAR_WIDTH = 24)`,
priority 1), messages (`Fill`, priority 2), reading (`Fill`, priority 0). Two
`Fill` columns split the content region evenly, so on a 120-column terminal the
list and the reading pane each get ~47. Below `MIN_PANE_WIDTH = 15` per column
the reading pane collapses first, then folders; the list is last to go.

### There is no body text to preview (why R1 dropped the preview line)

`EnvelopeSummary` is
`{ id, subject, from_display, from_addr, date_epoch_secs, flags, message_id, references }`
— headers only. The IMAP sync fetches `ENVELOPE_HEADER_FIELDS` (a
`BODY.PEEK[HEADER.FIELDS ...]`), never body text; the cache schema (v3) has no
body column; `MailStore` holds envelopes only. Bodies arrive one at a time
through `MailCommand::FetchMessage`, issued when a message is opened — and
`specification.md` states the contract explicitly: "loading is explicit, so
browsing the list never fetches."

So a preview line is not a rendering change with a data tail; the data does not
exist anywhere in the system today, and inventing it touches the IMAP fetch, the
maildir summarizer, the cache schema, and the sync cost model.

Two smaller facts that the card layout makes visible: `from_display` is empty
whenever the sender sent no display name (the table shows a blank From cell
today, which is easy to miss and would be a blank first line in a card), and the
thread indent plus the collapsed-children badge (`↳ `, `[+3]`) are currently
baked into the subject string.

## 2. Proposal

Revised after R1: the body preview is dropped — its cost was a sync-time body
fetch, a cache migration and a bandwidth regression for one line of text — so
the card is three header lines and nothing in `nitidus-mail` changes. This is
now a UI-crate feature end to end.

### 2.1 A row is a card of `ROW_HEIGHT` lines

```
R Alice Example
  Quarterly report
  Mon, 22 Jul 2026 15:04
```

- **Line 1** — sender: `from_display`, falling back to `from_addr` when the
  display name is empty (a blank first line is worse than an address).
- **Line 2** — subject, carrying the thread indent and the `[+3]` collapsed
  badge exactly as the table does today.
- **Line 3** — the date, in full: it owns a whole line, so it does not need the
  table's abbreviations.

A two-column state gutter (one glyph plus a space) runs down the left of the
card; §2.2 defines it. Each line is fitted (padded or ellipsis-truncated) to
the content width, so a card is a solid three-line block that styling can
paint.

`ROW_HEIGHT` is a constant of the layout (3), not a per-row measurement: every
card is the same height, so row-to-line arithmetic stays multiplication instead
of a prefix sum.

### 2.2 State: styling, plus one gutter glyph

Per R1 A4, unseen and flagged are carried by the theme roles — an unread
message is a bold card, a flagged one is tinted — across all three lines. But
answered (`R`), draft (`d`) and the batch mark (`*`) have no styling of their
own, and dropping the flag cell would silently drop them, so the card keeps a
two-column gutter to the left of all three lines.

The gutter holds at most one glyph, on the card's first line, chosen by a fixed
precedence: `*` (marked) > `D` (deleted) > `d` (draft) > `R` (answered) > blank.
Unseen and flagged never claim it — they are already visible as styling — so
the gutter is blank on ordinary mail and the card stays quiet.

Considered and rejected: a glyph per line (three states at once, but cryptic),
and a right-aligned suffix on the subject line (no fixed width cost, but it
collides with the subject and does not align down the list). The gutter is
cheap to revisit if a blank two-column strip proves wasteful at a narrow
list.

The table layout keeps its flag column unchanged.

### 2.3 Date form

A new `DateFormat::Full` renders `%a, %d %b %Y %H:%M` — `Mon, 22 Jul 2026
15:04`, always 22 characters whatever the date. Cards read `auto` as `Full`;
the table keeps reading `auto` as its three recency tiers. An explicit
`[ui.index] date` of `time`/`short`/`iso`/`full` applies verbatim in either
layout, so the knob that exists today keeps working rather than being silently
ignored by cards.

22 plus the gutter makes 24 the narrowest list a card can usefully take. A
narrower one truncates the date; that is the user's choice to make via `width`,
or by setting `date = "short"`.

### 2.4 Line hierarchy

Three lines of one weight read as a wall of text, so a card's lines carry
emphasis of their own: `ThemeIndexStyles` gains `sender` (a lifted foreground)
and `date` (the palette's existing disabled/dim foreground), with the subject
left at the row's normal weight in between. The lift is deliberately large —
the seed foreground is already near white, so a small one is invisible.

Emphasis is not bold: `unseen` already owns bold, and an unread card must stay
distinguishable from a read one. It patches over the row's base but _under_ the
flag roles, so a flagged card is amber on all three lines and a deleted one is
dim throughout — state outranks typography.

### 2.5 Striping

`ThemeIndexStyles` gains a `stripe` style: alternate cards take it as their base
background across the whole three-line block, gutter included, keyed on the
absolute row index so the banding does not shimmer while scrolling. It sits at
the bottom of the precedence chain — selection, hover and marks all paint over
it — and the tailwind-dark preset maps it to a subtle lift off the pane
background.

### 2.6 Row height plumbing

Row height becomes a property of the layout (`table` → 1, `cards` → 3) that
every piece of row arithmetic converts through:

- viewport rows = `area.height / row_height` — what `scrolled_top`, page motions
  and the window builder consume;
- `render_index` flat-maps each row into its lines and takes
  `viewport_rows * row_height` lines;
- `absolute_row` = `window_top + local_row / row_height`;
- the search highlight runs per line, over each of the card's lines, instead of
  over one fitted line.

### 2.7 Pane budget

The message list stops being a `Fill` column and becomes
`Fixed([ui.index] width)`, default 36, with the reading pane taking `Fill` and
therefore all the slack. Priorities are unchanged: reading still collapses
first, then folders. The table layout keeps today's even `Fill`/`Fill` split.

36 minus the two-column gutter leaves 34: comfortably more than the full date
needs, and enough sender and subject to scan without opening anything. The
reading pane still ends up far wider than an even split would give it, which is
the point of fixing the list at all. (Revised in R3 — the default was 24, which
fit the date exactly and truncated almost every sender.)

### 2.8 The table layout stays

`[ui.index] layout = "cards" | "table"` selects between them; `columns` and its
built-in widths continue to describe the table only. Default: `cards`.

Out of scope: the body preview line (dropped in R1), automatic fallback to the
table on narrow terminals (deferred in R1 Q9), multi-line subject wrapping,
per-row quick actions on hover, avatars/initials, attachment or importance
indicators, and pattern-driven per-line colors (still phase 3 of
`feature-index-custom-v1`).

## 3. Discussion

### 3.1 R1 Questions

1. **Preview cost.** Fetching a body prefix for every envelope makes the first
   sync of a large folder materially heavier (200 chars/message stored, but the
   IMAP fetch pulls a fixed byte prefix per message — a 10k-message folder is
   tens of megabytes at 2 KB each). Is that acceptable as the default, should it
   be opt-in, or should the prefix be smaller (e.g. 512 bytes)?
2. **Missing previews.** Until a folder re-syncs after the v4 migration — and
   for HTML-only messages if we do not strip HTML — the third line is blank.
   Blank line (fixed 3-line cards, simple arithmetic), or collapse to a 2-line
   card per message (variable heights, prefix-sum arithmetic everywhere)?
3. **HTML-only mail.** A lot of real mail has no `text/plain` part. Strip the
   HTML prefix to text for the preview (needs the HTML tier-1 path in the
   sync/summarize layer, in the mail crate), or accept an empty preview for
   those messages in v1?
4. **Flags placement.** Card line 1 is `sender ................ flags`. Would
   you rather have flags as a left prefix (`N  Alice Example`), keep a fixed
   gutter column to the left of all three lines for state, or drop the flag
   letters and let theme styling carry unseen/flagged?
5. **Date form.** Is `Mon 7/22` fixed US m/d, or should this follow a
   configurable form (`Mon 22/7`, `Mon Jul 22`)? And does today's mail show a
   time (`3:15 PM`) instead of a weekday, as Outlook does?
6. **Spacing.** Three lines flush against the next card, or a blank fourth
   separator line (`ROW_HEIGHT = 4`) for breathing room? Flush is denser; the
   selected-background block is what separates cards visually.
7. **Search and limit scope.** Should `:limit` and incremental search match
   against the preview text as well as sender/subject? (Matching it means the
   highlight can land on the third line, which the per-line highlight handles.)
8. **List width.** Is `Fixed(48)` the right list width, should it be
   configurable (`[ui.index] width`), or should it stay proportional (e.g. 40%
   of the content region) so wide terminals give the list more?
9. **Narrow terminals.** Below roughly 30 columns a card is mostly ellipsis.
   Fall back to the single-line table automatically at some threshold, or let
   the card degrade?
10. **Default layout.** Do cards become the default for everyone (the proposal),
    or does the table stay the default with cards opt-in?

### 3.2 R1 Answers

1. Cost is not worth it, let's drop text preview and just do 3 lines
   sender/date/subject
2. N/A
3. N/A
4. Let's use styling to denote unread flagged.
5. Keep full date since it will be it's own line
6. flush, but can we add striping?
7. N/A
8. Let's make it configurable, and set at 24 for default
9. Let's deal with that later
10. cards default

### 3.3 R2 Questions

1. **Line order.** The proposal takes A1 literally — sender, date, subject. In
   Outlook the subject sits second because that is what the eye scans, with the
   date pushed to the end. Keep sender/date/subject, or sender/subject/date?
2. **What "full date" means.** Proposed: `Mon, 22 Jul 2026 15:04` (22 chars — it
   fits a 24-wide list with two columns to spare). Shorter alternatives if that
   is too tight: `Mon 22 Jul 15:04`, or keeping today's `auto` tiers now that
   the date has its own line. Does `[ui.index] date` still apply to cards, or
   does the card always use the full form?
3. **Answered, draft, deleted.** Dropping the flag letters costs the `R`
   (answered) and `d` (draft) indicators, which have no theme role today —
   deleted does (dimmed). Add `answered`/`draft` roles to the theme, keep a
   single trailing state character on the sender line, or accept that cards do
   not show those two states in v1?
4. **Striping scope.** Stripe every other card's full three-line block from a
   new `index.stripe` theme role, sitting under selection/hover/marked in the
   precedence chain — confirm that is what you want, rather than (say) striping
   only the subject line or using a leading rule character.

### 3.4 R2 Answers

1. agreed, sender/subject/date
2. proposed
3. ahh, good point, I hadn't thought of that. Let's keep a gutter, unless you
   have a better suggestion?
4. stripe whole block

### 3.5 R3 — the default width, after seeing it

Post-implementation, pre-merge: the default list width goes from 24 to 36. At
24 the full date filled the content area exactly and everything else truncated
hard; 36 keeps the date intact and leaves 34 columns for the sender and
subject, while the reading pane still takes the rest. 24 remains the narrowest
useful setting, and is now recorded as such rather than as the default.

## 4. Plan

Each phase leaves the workspace compiling with tests green. Nothing outside
`crates/nitidus` and `crates/nitidus-ui-kit` is touched.

**Phase 1 — theme role.** Add `stripe` to `ThemeIndexStyles` and map it in the
tailwind-dark preset. No consumer yet; preset tests assert the new role.

**Phase 2 — config surface.** `[ui.index] layout` (`cards` | `table`, default
`cards`), `[ui.index] width` and the `DateFormat::Full` variant in
`config/schema.rs`, with load-time validation, `example-config.toml` entries and the strict-parse tests.
Nothing reads them yet.

**Phase 3 — row height as a parameter.** Thread a `row_height` through the
index: viewport rows in `refresh_index`, the window builder, `render_index`,
page motions, and `mouse::absolute_row`. Hard-code it to 1 in this phase, so
behavior is provably unchanged; unit tests cover the conversions at heights 1
and 3.

**Phase 4 — the card renderer.** `render.rs` grows a card path beside
`row_line`: the state gutter, three fitted lines, theme roles patched across all
of them, striping by absolute row parity, and per-line search highlighting. The
layout config selects the path and supplies `row_height`. Row-formatting unit
tests cover
fitting, truncation, styling precedence and highlight placement.

**Phase 5 — pane budget.** `panes.rs` takes the list width from config: a
`Fixed(width)` messages column under `cards`, today's `Fill` under `table`.
Existing `panes.rs` tests extend to both layouts and to the collapse order at
narrow widths.

**Phase 6 — docs and verification.** Update `specification.md`'s index bullet
and `documentation/example-config.toml`, then the full clippy + workspace test
run for §5.

## 5. Verification

```bash
cargo clippy --workspace --all-targets   # clean, no warnings
CARGO_INCREMENTAL=0 cargo test --workspace
```

654 tests pass, 0 failed, 0 ignored, across the whole workspace (375 in the
`nitidus` lib, 33 in `nitidus-ui-kit`, and every integration suite).

Behavior preservation for the table layout was proven by construction rather
than by inspection. Phase 3 introduced `row_height` as a parameter threaded
through every viewport, page and click calculation while leaving it pinned at
1: the full suite passed unchanged at that commit, so the arithmetic conversion
is provably neutral. The card layout then only had to supply a different
divisor. The pre-existing table tests were kept verbatim (retargeted at an
explicit `layout: Table` context) and still pass, including
`default_columns_fill_exact_width_in_the_established_order`.

The layout that a user actually sees is verified at the buffer level, not by
eye: `index/tests.rs` draws `render_index` into a ratatui `TestBackend` and
asserts the exact cell contents — three stacked lines per message, the next
card starting flush, nine lines holding exactly three cards, a pane too short
for a whole card drawing none of it, and the table still drawing one line per
message. The app itself was not launched; the buffer assertions cover the same
output a screenshot would show for the index.

Two invariants from the design are pinned by tests so they cannot drift:

- `card_lines` returns exactly `IndexLayout::Cards.row_height()` lines — the
  renderer and the scroll arithmetic cannot disagree.
- The full date is 22 characters for every date, so it survives intact down to
  a 24-wide list — the narrowest a card can usefully take.
- A card's sender line is brighter than its subject and its date line dimmer,
  and a flag tint still overrides both on all three lines.

## 6. Implementation Report

Delivered in the six planned phases, each left compiling with tests green.

**What changed.** `ThemeIndexStyles` gained a `stripe` role (tailwind-dark
lifts the pane background by 0.05, well under the focused state's 0.125) and
the `sender`/`date` emphasis pair for a card's line hierarchy.
`[ui.index]` gained `layout` and `width`, and `DateFormat` gained `Full`.
`index/render.rs` became `index/render/` — `mod.rs` (row model, styles,
dispatch, style precedence), `table.rs`, `card.rs`, `date.rs`, `text.rs` — and
`panes.rs` now takes a `PaneBudget { sidebar_visible, list_width }` instead of
a bare bool.

**Two deviations from the plan, both deliberate:**

1. **`render.rs` was split, not extended.** It was already 494 lines — well
   past the 300-line limit in `rules/code.md` — and cards would have pushed it
   past 600. Per that rule's instruction to fix such problems while touching
   the code, it became a five-file module, each file well under the limit. The
   index's own new render tests likewise live in `index/tests.rs`, following
   the existing `overlay/form/tests.rs` precedent.

2. **`IndexRow` now carries `flags: Flags` instead of mirrored booleans.** It
   previously held `unseen`/`flagged`/`deleted` bools plus a pre-rendered
   `flag_cell` string. The card needs answered and draft too, and adding two
   more mirror bools would have made the duplication worse, so the row now
   holds the flag set itself and both layouts derive what they need from it
   (the table builds its flag cell at render time; the card picks its gutter
   glyph). Net effect: fewer fields, one source of truth, no behavior change.

**Worth knowing.** The sender fallback to `from_addr` is card-only — the
table's From column still renders blank for a nameless sender, exactly as
before, since widening that was not in scope. Striping is likewise card-only:
`striped` is set only under the card layout, so the table is untouched.
`[ui.index] width` is not validated at load; the pane budget's `MIN_PANE_WIDTH`
floor clamps anything smaller, which a test pins. That avoided duplicating the
layout's minimum into the config layer.

**Follow-ups, none blocking:**

- Narrow-terminal fallback to the table (deferred in R1 Q9) is still open; a
  card at 15 columns is mostly ellipsis.
- `width` is only meaningful under `cards`; it is silently ignored under
  `table`. A load-time notice could say so.
- If a blank two-column gutter proves wasteful at a narrow width, §2.2 records
  the rejected alternatives to fall back on.

## 7. Testing and Cleanup

The cleanup skill ran over the feature's scope (`index/`, `panes.rs`,
`config/schema.rs`, `theme/`). Findings were small, which is expected for
newly written code:

- Removed the `resolve_date` wrapper in `render/mod.rs` — a one-line
  indirection over `date::resolve` carrying a second doc comment that said the
  same thing as the first. It is now a re-export, so there is one definition
  and one explanation.
- Shortened the `DEFAULT_INDEX_WIDTH` doc comment, which restated the design
  doc rather than the constraint on the constant.
- Tightened visibility now that the module boundary is real: `table::flag_cell`
  is private (grep proved no callers outside its file) and `date::format_date`
  is `pub(super)`.
- No dead code: `cargo build --workspace` and `cargo clippy --workspace
--all-targets` report no `dead_code`, unused or never-read warnings.

Post-cleanup verification: `cargo fmt --all`, clippy clean, and 654 tests
passing with 0 failures.
