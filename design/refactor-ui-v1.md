# refactor - UI - v1

A broader interaction-surface refactor for nitidus, collecting the UI-shape
ideas that outgrew keybinding work. Parked during refactor-keymap-v1
(2026-07-25) and unparked now that feature-overlay-forms-v1 has landed the modal
machinery it was waiting on.

The three headline items — a preview pane, confirmations as overlays, and
feedback via toasts — are each individually small. What makes this a refactor
rather than three features is that all three land on the same handful of shared
mechanisms: pane focus, the modal stack, layout, and the statusline. Section 1
is therefore an inventory of every UI surface in the app and what this work does
to it, so the shared mechanisms can be designed once instead of three times.

## 1. Current Design

### 1.1 The three observations that spawned this doc

- **The pager is a screen, not a pane**: it replaces the index in the content
  region (`Screen` enum), with explicit open/fetch/close semantics. The sidebar
  is the only side-by-side pane in the mail tab.
- **Feedback is statusline-bound**: y/n confirmations run through the bottom-row
  prompt, and errors/notices land in the statusline's center segment (with the
  toast plugin so far used sparingly). Destructive confirms, multi-line errors,
  and anything that should interrupt visually all compete for one line of text.
- Overlay machinery exists (picker panels, the explorer modal, completion
  panels, and now stepped forms) and the theme system provides styled surfaces —
  the building blocks for richer modals are present but underused.

### 1.2 Inventory

Every drawn surface in the app, grouped by how this refactor reaches it. The
tiers are dependency-ordered: tier 0 is what tiers 1–5 all consume.

#### Tier 0 — Cross-cutting infrastructure

Not screens, but nothing below can move until these do.

| Component          | Files                                                                                | Why it is in scope                                                                                                                                                                        |
| ------------------ | ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Shell layout fns   | `ui-kit/src/layout.rs`                                                               | Only knows `sidebar_split` — two columns. A miller layout needs an n-column budget with collapse rules for narrow terminals.                                                              |
| Elevation ladder   | `ui-kit/src/layer.rs`                                                                | Correct and complete, but `MODAL` currently has one occupant (the attach preview). Confirmations claim that rung, which is the case the ladder was built for.                             |
| Theme palette      | `ui-kit/src/theme/{palette,states}.rs`                                               | `ThemePalette` carries `error`/`info`/`success`/`warning`, but no severity currently routes through them consistently. The `base`/`paper` split has to hold for confirmations and toasts. |
| Router modal gates | `router.rs:82-110`                                                                   | Nine sequential gates in a hand-ordered stack. Confirmations make ten. The order is still an undocumented convention with nothing tying it to `layer`.                                    |
| Keymap contexts    | `keymap/{mod,defaults}.rs`                                                           | `CONTEXT_*` is keyed to `Screen`. If the pager becomes a pane, context must follow _focus_ instead. A `confirm` context is new.                                                           |
| `Screen` enum      | `screen.rs`                                                                          | The premise of item 1: `Pager` stops being a screen. `MailScreenMemory` exists only to restore a mail-tab screen and may lose its reason to exist.                                        |
| Mouse hit-testing  | `mouse.rs`, `{index,sidebar,contacts,explorer,overlay/picker,overlay/form}/mouse.rs` | `mouse.rs` already special-cases "picker, the file explorer, or a y/n prompt" — that list changes on every item below.                                                                    |

#### Tier 1 — Mail tab panes

The miller-column item.

| Component           | Files                                                                                | Verdict                                                                                                                                                                                                                                                  |
| ------------------- | ------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Sidebar             | `sidebar/{mod,render,ops,tree,mouse}.rs`                                             | Becomes column 1 of 3. `SidebarState.focused` is a bool — the mail tab's only pane-focus concept — and must generalize.                                                                                                                                  |
| Index               | `index/{mod,render,view,ops,mouse,marks,search,filter,thread_view,staged,remove}.rs` | Column 2. `render.rs` is the app's widest file at 484 lines; the configurable column budgets from feature-index-custom-v1 now compete with a third pane for width.                                                                                       |
| Pager               | `pager/{mod,render,ops,body,html,peek,save}.rs`                                      | Column 3. Open/fetch/close becomes selection-driven, needing debounce and stale-fetch cancellation. `peek.rs` is _not_ a preview mechanism (see §1.4) — it is the SEEN-flag policy, and a following preview breaks its trigger rather than replacing it. |
| Statusline segments | `shell.rs`, and the `IndexStatus` / `PagerStatus` / `ContactsStatus` resources       | Three per-screen status resources feed one line. With three panes visible at once, which one owns the segment is undecided.                                                                                                                              |

#### Tier 2 — Contacts tab

Already a two-pane list-and-detail surface with its own `PaneFocus`. It is the
closest existing precedent for the miller layout and should converge on it
rather than stay bespoke.

| Component                | Files                                     | Verdict                                                                                                             |
| ------------------------ | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Contacts view and focus  | `contacts/view.rs`                        | `PaneFocus { Table, Detail }` is a second, parallel pane-focus mechanism. Merges with the mail tab's.               |
| Contacts drawing         | `contacts/{draw,render,photo}.rs`         | Splits its own content region with a hardcoded `TABLE_PANE_PERCENT`; does not go through `ui-kit::layout` at all.   |
| Contact property editors | `contacts/{add,edit,mutate}.rs`           | Nine prompt call sites — see tier 4.                                                                                |
| Import / export          | `contacts/transfer.rs`, `explorer/mod.rs` | A path prompt plus the file explorer, which has hardcoded keys and no rebindable context, unlike every other modal. |

#### Tier 3 — Feedback surfaces

The toast item.

| Component         | Files                          | Verdict                                                                                |
| ----------------- | ------------------------------ | -------------------------------------------------------------------------------------- |
| `StatusMessage`   | `status.rs`                    | 76 references across 30 files. Needs a severity-to-destination policy, not a rename.   |
| Toast layer       | `toast.rs`                     | Currently mirrors only the outbox countdown. Becomes the sink for warnings and errors. |
| Statusline render | `shell.rs::refresh_statusline` | Stops hosting events; keeps state.                                                     |
| Chord hint        | `router.rs::PendingKeys::hint` | Shares the center segment it is about to inherit outright.                             |

Heaviest writers, which is where the routing policy actually bites:
`compose/drafts.rs` (7), `compose/persist.rs` (5), `accounts/oauth.rs` (5),
`accounts/mod.rs` (5), `compose/recall.rs` (4), `accounts/wizard/mod.rs` (4).

#### Tier 4 — Bottom-bar prompts

feature-overlay-forms-v1 shipped the form subsystem and migrated exactly two
callers (the account wizard and `:set-password`), recording the rest as
follow-ups for this document. Twenty `open_prompt` call sites across nine files
remain.

**Confirmations (7).** These are item 2 below. They want a `Confirm` overlay,
not a `FormSpec` — a question, context about what is being acted on, and two
buttons.

| Site                    | Question                                |
| ----------------------- | --------------------------------------- |
| `index/remove.rs:133`   | delete N messages permanently           |
| `index/remove.rs:196`   | delete the selected message permanently |
| `compose/ops.rs:225`    | discard the message                     |
| `compose/drafts.rs:142` | send without a subject                  |
| `compose/drafts.rs:159` | send without the attachment referred to |
| `contacts/add.rs:247`   | delete the selected contact             |
| `accounts/manage.rs:26` | remove the named account                |

**Data entry (13).** Candidates for `FormSpec`, several of which are chains that
collapse into one multi-field form the way the wizard's thirteen steps did.

| File                   | Sites | What they ask for                                                   |
| ---------------------- | ----- | ------------------------------------------------------------------- |
| `contacts/add.rs`      | 6     | new-contact name and email, property name and value, raw vCard line |
| `compose/ops.rs`       | 4     | header edits (To, Cc, Bcc, Subject)                                 |
| `contacts/edit.rs`     | 2     | property value, raw vCard line                                      |
| `compose/drafts.rs`    | 1     | attachment path                                                     |
| `compose/reply.rs`     | 1     | forward recipient                                                   |
| `contacts/mutate.rs`   | 1     | property mutation                                                   |
| `contacts/transfer.rs` | 1     | export path                                                         |

Once both groups move, `prompt/mod.rs` (439 lines) and `prompt/panel.rs` become
deletable, and `InputMode::Prompt` and its router gate go with them. That is the
end state overlay-forms §2.7 named: the bottom row keeps the statusline, the `:`
command line, and incremental `/` search.

#### Tier 5 — Existing modal surfaces

| Component                         | Files                                    | Verdict                                                                                                                                                         |
| --------------------------------- | ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Picker                            | `overlay/picker/{mod,render,mouse}.rs`   | The reference implementation. Mostly a donor to the confirm overlay.                                                                                            |
| Form                              | `overlay/form/` (11 files)               | Machinery is done. Gains a `Confirm` sibling, and the deferred `FieldSpan::Half` becomes worth building once contact and compose forms exist.                   |
| Explorer                          | `explorer/mod.rs`                        | The outlier: hardcoded keys, no keymap context, its own resource and gate. Should become a citizen of whatever the overlay subsystem settles into.              |
| Attach preview                    | `compose/preview.rs`                     | Any-key dismiss at `MODAL`; the current sole occupant of the rung confirmations want.                                                                           |
| Help overlay                      | `help.rs`                                | Built on the picker; inherits whatever the picker becomes.                                                                                                      |
| Command line and completion panel | `cmdline/{mod,panel}.rs`                 | Stays on the bottom row by design. Its `PANEL` rung must survive the reshuffle.                                                                                 |
| Incremental search                | `index/search.rs`                        | Renders into `statusline_layout` and deliberately stays there (vim and less precedent) — but it is the reason the statusline cannot simply be reduced to state. |
| Compose review and inline editor  | `compose/{render,inline,token,style}.rs` | Untouched by items 1–3, but it is the only screen with no sidebar (`main_layout`), so it is the exception any layout rework has to accommodate.                 |

### 1.3 Three problems the original stub did not account for

1. **Two pane-focus mechanisms already exist** — `SidebarState.focused` (bool)
   and `ContactsView.focus` (`PaneFocus`) — and item 1 implies a third. Unifying
   them is a prerequisite for the preview pane, not a follow-on to it.
2. **The contacts tab never adopted `ui-kit::layout`.** It splits its own
   content region with a local percentage. A miller layout covering only the
   mail tab would leave the app with two layout systems rather than one.
3. **Item 3 is the largest item by call-site count** (76 status references) and
   the least specified (three lines in the stub). It needs a severity policy
   before any file is touched.

### 1.4 What a following preview actually costs

Established while answering R1 Q5; both facts were misstated or absent in the
first draft of this document.

**`peek.rs` is not a preview.** It is the deferred-SEEN timer from
feature-comfort-v1: `[ui.pager] mark_read` is `Open` (flag on open), `Never`, or
`After(delay)`, and `PeekTimer` arms on open, disarms on close, and fires only
while the same message is still open. The name refers to peeking at a message
_without marking it read_. It has no bearing on fetching, and nothing about it
is a partial load. It therefore neither subsumes nor is subsumed by a preview
pane — but a preview that follows selection destroys its trigger, because
"opened" stops being an explicit act. Under `MarkRead::Open`, arrowing down the
index would flag every message it passed.

**There is exactly one fetch path, and no body cache.**
`pager/ops.rs:: open_selected` always sends `MailCommand::FetchMessage` for the
whole message; `adjacent` (`J`/`K`) re-enters the same path, so even returning
to a message just read refetches it. The sqlite cache
(`nitidus-mail/src/cache/`) stores folders, envelopes and harvested addresses —
no bodies. On the maildir backend a fetch is a local file read and this is free;
on IMAP it is a network round trip.

Together these would make debounce and stale-fetch cancellation load-bearing
rather than nice to have: a preview following an index cursor turns twenty rows
of `j` into up to twenty full message fetches over the network. R2 A2 resolved
this by not following the cursor at all — the preview loads only on an explicit
`Enter` or `→` — which removes the debounce and cancellation problem outright
and leaves the existing job-token discipline (`PagerState` ignores a response
whose job is no longer current) sufficient as it stands.

## 2. Proposal

Settled across R1–R3. The three headline items the stub named survive as items
§2.3, §2.5 and §2.6; the rest is what the inventory and the discussion added.

### 2.1 Focus replaces `Screen`

`Screen` is deleted. What replaces it is a tab (already in `Tabs`) plus a
`Focus` resource naming the focused pane within that tab:

```rust
#[derive(Resource)]
pub struct Focus(pub Pane);

pub enum Pane { Folders, Messages, Reading, ContactList, ContactDetail }
```

`SidebarState.focused` (a bool) and `ContactsView.focus` (`PaneFocus`) both
collapse into it — the two parallel mechanisms §1.3 flagged. The keymap context
derives from `Focus` rather than `Screen`, so `CONTEXT_PAGER` comes to mean "the
reading pane has focus" and `CONTEXT_SIDEBAR` stops being a special case bolted
on ahead of the others. `MailScreenMemory` disappears with `Screen`; a tab
remembers its own focus instead.

`Screen` shrinks rather than vanishing at once: `Pager` goes when the reading
pane lands (§2.3), `Compose` when the composer becomes an overlay (§2.8), and
the enum is deleted when the last variant leaves.

### 2.2 Layout: n columns with a minimum-width collapse rule

`ui-kit/src/layout.rs` gains an n-column budget replacing `sidebar_split`. Each
column declares a preferred and a minimum width; columns that cannot meet their
minimum collapse, lowest priority first. Per R1 A4 the reading pane collapses
before the folder sidebar.

Concrete budget for the mail tab: folders at `SIDEBAR_WIDTH` (24), messages
filling, reading filling. `MIN_PANE_WIDTH` governs collapse. When the reading
pane is collapsed, `Enter` opens the reading overlay (§2.3) instead — which is
the same escape the composer uses at narrow widths (§2.8), so one rule covers
both.

A one-column gutter sits between neighbouring panes, carrying a vertical rule.
Without it the columns abut and there is nothing to tell the eye where one
pane's text ends and the next begins. The gutter is part of the budget, so it
is reserved before the columns are sized and disappears with the pane it
separated; a single widget over the content region paints the rules, and
because no pane owns those cells nothing can draw over them.

### 2.3 The reading pane, and loading only on request

Three columns: folders | messages | reading. The reading pane does **not**
follow the index cursor. §1.4 is why: there is one fetch path, no body cache,
and on IMAP every fetch is a network round trip, so a following preview turns
twenty `j` presses into twenty fetches.

Loading is therefore explicit, on `Enter` or `→` from the message list, or on
the hotkey that opens the reading overlay. The reading pane tracks its own
loaded message independently of the index selection, which means the two can
disagree — and per R3 A2 the index shows which row is loaded, with a marker in
the themed per-row styling feature-index-custom-v1 already provides. Before
anything is loaded the pane shows a short help line naming the navigation and
`Enter`.

Debounce and stale-fetch cancellation are **not** needed: nothing fetches
without a keypress, and `PagerState` already ignores a response whose job is no
longer current. The body cache considered in R2 is dropped from this document
(R3 A3) and recorded as a `nitidus-mail` follow-up. `pager/render.rs` already
renders `loading…`.

**The reading overlay** is the full-screen read: full content height less a
margin, width `min(content_width, MAX)` centered, with `MAX` a new config key
under `[ui.pager]`. Closing returns to the message list with the selection
unchanged; there is no independent cursor inside it, because walking messages
is what the reading pane is for (R2 A6).

`Z` from the message list opens the selected message before zooming — an empty
pane has nothing to enlarge — so it reads as "read this one full screen" from a
single keystroke. From the reading pane it is a plain toggle. Opening a message
the pane already holds skips the fetch and just moves focus.

It draws on `layer::ZOOM`, a rung between `BASE` and `PANEL`, rather than at
`MODAL` as this section first said. `:links` opens a picker from the reading
pane, and a pane at `MODAL` would have covered the picker it spawned. It is a
raised pane, not a modal: it never joins the overlay stack or takes the
keyboard, and every panel, picker, form and confirmation still draws above it.
Being raised, it clears the region and frames itself with the message's
subject — otherwise the panes underneath show through wherever a line falls
short of the width.

**peek is deleted.** `peek.rs`, `PeekTimer` and `MarkRead::After` all go: once
loading is an explicit act there is nothing left to defer. `mark_read` keeps its
other axis as `"open" | "never"`, where `"open"` now means "when a fetch
completes into the reading pane or the reading overlay". An existing
`config.toml` carrying a numeric `mark_read` is coerced to `"open"` with a
startup notice rather than refused (R3 A1). This is a specification change —
`specification.md:49` and `roadmap.md:27` both list mark-read delay.

### 2.4 One overlay surface

Per R2 A5, option (b): a single `OverlaySurface` abstraction with one router
gate and an explicit stack, replacing the hand-ordered ladder of gates at
`router.rs:82-110`. Picker, form, confirm, explorer and the message log each
become an implementation, sharing frame, title, focus ring, mouse handling and
their rung on `layer`. The stacking rule §1.2 tier 0 calls an undocumented
convention becomes a property of the stack: what is pushed last is drawn above
and gets the keyboard.

The explorer stops being the outlier — it gains the rebindable context every
other modal has.

### 2.5 Confirmations as overlays

The seven y/n sites in §1.2 tier 4 become a `Confirm` surface on the
`overlay/form/` machinery at `layer::MODAL`: a question, room for context about
what is being acted on, and two buttons. Focus starts on the safe option and
`Esc` cancels, so a reflexive `Enter` never destroys anything.

### 2.6 Feedback: toasts, a message log, and what the statusline keeps

The severity cut from R1 A6: `Error` and `Warning` surface as toasts, `Info`
stays on the statusline, and anything carrying a follow-up action is a confirm
overlay instead of a message.

`StatusMessage` becomes a bounded ring buffer of `(elapsed, severity, text)`.
Toasts show the tail transiently; the buffer is the durable record, readable in
a **message log** — a togglable overlay sliding up in the bottom-right quadrant
(R2 A3), built on the shared surface of §2.4 so filtering and scrollback come
free.

The statusline keeps the tab name, a position/total, the pending-chord hint,
`Info` messages, and the version — and remains where `:` commands are typed and
`/` search runs. Position/total stays index-owned regardless of which pane has
focus (R3 A4).

### 2.7 The bottom bar's end state

All twenty remaining `open_prompt` call sites migrate: the seven confirms to
§2.5, the thirteen data-entry sites to forms. `prompt/mod.rs`,
`prompt/panel.rs`, `InputMode::Prompt` and its router gate are then deleted,
completing what overlay-forms §2.7 named. The bottom row ends as the
statusline, the `:` command line, and incremental `/` search.

### 2.8 Compose as an overlay

The composer stops being a screen. Per R2 A8 and R3 A6, a reply opens in the
reading pane with the index still on the message being replied to, and a new
composition opens as an overlay.

One surface, not two: the compose overlay holds the header form, the body
editor and the review as pages of the same stepped form (R3 A6.2, A6.3).
Splitting headers into a form that closes into a separate body pane would give
Cancel two meanings and make postpone/recall capture a half-filled state.

`compose/inline.rs` is a ratatui-textarea session and needs width. The minimum-
width rule of §2.2 covers it: below the threshold a reply cannot use the reading
pane and goes to the overlay instead. `refuse_while_composing` in `shell.rs` is
dropped (R3 A6.4) — drafts already survive tab switching via postpone/recall,
so the guard costs more than it protects.

### 2.9 Contacts: ported, not relaid out

R3 A5. Contacts is touched by three decisions it cannot escape — `Screen`
removal, focus unification, and the nine prompt call sites in
`contacts/{add,edit,mutate,transfer}.rs`. All three land here. What defers to a
successor is only its *layout*: `contacts/draw.rs` keeps its local
`TABLE_PANE_PERCENT` split rather than moving onto the column budget of §2.2.

### 2.10 Assumptions and out of scope

Three details went unchallenged in discussion and are taken as settled; call
them out if any is wrong:

- The message log is built on the picker rather than hand-rolled, and holds 200
  entries with no persistence across runs.
- `MIN_PANE_WIDTH` is a constant, not config; only the reading overlay's `MAX`
  becomes a config key.
- New default bindings (reading overlay, log toggle, pane focus cycling) are
  chosen during implementation and listed in §6, since every context is
  rebindable.

Out of scope: a message body cache (R3 A3, a `nitidus-mail` follow-up); the
contacts layout migration (§2.9); and the phase 2 keymap items — leader menus,
which-key hints, selectable keymap schemes — that a redesigned surface would
otherwise invite.

## 3. Discussion

### 3.1 R1 Questions

1. **Scope.** The inventory shows three items that share tier 0 but are
   otherwise independent, plus a fourth (the remaining data-entry prompts and
   the deletion of `prompt/`) that is currently unowned. Options: all four in
   this document; the three headline items here and prompts in a successor; or
   split into three documents sharing a tier-0 prerequisite chore. Which?
2. **Pane focus.** §1.3 argues the two existing focus mechanisms must unify
   before the preview pane can land. Is that a phase 1 of this document, or a
   separate refactor doc that this one depends on?
3. **The pager's identity.** Item 1 asks whether the pager becomes a pane focus
   rather than a `Screen`. If it does, what happens to full-screen reading — a
   zoom/maximize toggle on the preview pane, a retained `Screen` for the
   explicit open, or no full-screen pager at all?
4. **Narrow terminals.** Three columns at 80 wide leaves roughly 26 each.
   Collapse rule preference: drop the sidebar first, drop the preview first, or
   a configured minimum width per pane that collapses whatever does not fit?
5. **`peek.rs`.** The peek feature from feature-comfort-v1 is a partial preview
   already. Does the preview pane subsume and delete it, or do they coexist
   (peek as the transient look, preview as the persistent pane)?
6. **Severity policy.** Proposal to react to: `Error` and `Warning` go to
   toasts, `Info` stays on the statusline, and anything with a follow-up action
   becomes a confirm overlay. Or a different cut?
7. **Statusline ownership with three panes.** `IndexStatus`, `PagerStatus` and
   `ContactsStatus` currently take turns because only one screen is visible.
   With three panes visible, does the segment follow the focused pane, always
   show the index, or show a composite?
8. **The explorer.** It is the last hand-rolled modal. Normalize it in this
   document (it is small, and tier 0 touches its gate anyway), or leave it alone
   and record it as a follow-up?

### 3.2 R1 Answers

1. All in this document
2. phase 1 prereq, no separate doc.
3. my thought is 3 pane, folders/index/reading(preview), and if we want to
   provide a full screen reading, we could make it an overlay option, with the
   overlay taking most of the screen. We may want to add some terminal size
   detection, if the users is on a large screen we may want a centered overlay
   with a max width of ~80 cols or something like that.
4. drop preview first and make enter open in overlay as per #3.
5. Is there any difference from a network loading between peek and full load? If
   yes, then make peek the default on moving arrows to a new message, if it is
   loading the full message, then get rid of peek. When on the second pane
   (index), don't load the message unless enter or right arrow is pressed on the
   new message. Unless you disagree or have a better recomendation?
6. that works.
7. If that's the case we may want to rethink status line completely... does a
   togglable log make more sense? Like a popup panel that slides in from the
   bottom showing recent status messages? Or is that overkill? I think we do
   need a unified system for notifying the user of changes that aren't
   exclusively controlled by a UI component or pane.
8. Normalize both picker and explorer as much as possible to use this framework.

### 3.3 R2 Questions

R1 settled scope (all four items here), the pane-focus prerequisite (phase 1),
the three-pane shape with a full-screen reading overlay, preview-drops-first on
narrow terminals, the severity cut, and normalizing both picker and explorer.
What follows is what those answers opened up, plus one correction.

**Correction, ahead of Q1.** R1 Q5 asked whether peek differs from a full load
over the network. It does not, because peek is not a load at all — §1.4 records
what it actually is. Neither branch of the answer ("make peek the default on
arrow" / "get rid of peek") applies as written, so Q1 re-asks the decision that
question was really reaching for.

1. **`mark_read` under a following preview.** peek survives as a mechanism but
   loses its trigger: with a preview pane, "opened" is no longer an act the user
   performs. Options: (a) the preview never marks read, and `mark_read` applies
   only to the full-screen overlay — the SEEN clock starts on the explicit open;
   (b) the preview arms the timer, and `MarkRead::Open` is re-read as "once the
   fetch debounce settles", so idling on a row for a moment still marks it read;
   (c) preview marks read exactly as opening does today. I recommend (a): it
   preserves the current meaning of every `mark_read` value, and (c) is the
   behavior R1 Q5's own instinct was trying to avoid. But (b) is defensible if
   you want the preview to feel like reading rather than glancing.
2. **Fetch economics.** Given §1.4, a following preview needs at minimum a
   debounce (~150–250 ms) and cancellation of the in-flight job when the
   selection moves again. Beyond that: (a) debounce only, and accept one fetch
   per settled selection, including refetching a message you just read; (b)
   debounce plus an in-memory LRU of message bodies keyed by account/folder/id,
   bounded by count or bytes; (c) debounce plus bodies persisted in the existing
   sqlite cache. I recommend (b) here and (c) recorded as a follow-up — (c) is a
   `nitidus-mail` schema change with eviction, invalidation and disk-budget
   questions that do not belong in a UI refactor. Does (b) belong in this
   document, or is even that a mail-crate concern to split out?
3. **The message log.** R1 Q7's own suggestion, which I think is right and not
   overkill — it is `:messages` in vim and `*Messages*` in emacs, and it is the
   unified notification system that answer asked for. Proposed shape:
   `StatusMessage` becomes a bounded ring buffer of `(elapsed, severity, text)`;
   toasts surface the tail transiently per the R1 Q6 severity cut; a togglable
   panel shows the whole buffer. Three things to settle: does the panel slide in
   from the bottom as its own region (pushing the panes up) or open as a
   centered overlay at `layer::OVERLAY`; is it built on the picker so filtering
   and scrollback come free, at the cost of feeling like a palette rather than a
   log; and what buffer depth (proposal: 200 entries, no persistence across
   runs)?
4. **What the statusline keeps.** With events routed to toasts and the log, the
   remaining content is the tab name, a position/total, the pending-chord hint,
   and the version. R1 Q7 wondered whether that is enough to justify the row at
   all. If we keep it: does position/total follow the focused pane (folder n/m
   when the sidebar has focus, message n/m when the index does), or stay
   index-owned regardless of focus?
5. **How far "as much as possible" goes (R1 Q8).** Three readings, in rising
   order of cost. (a) **Shared chrome**: picker, form, confirm and explorer all
   draw the same frame, title, focus ring and mouse handling from one place, but
   keep their own state, resources and router gates. (b) **Shared surface**: one
   `OverlaySurface` abstraction with a single router gate and an explicit stack,
   each of the four an implementation — this is what makes the gate ordering in
   §1.2 tier 0 a stated rule instead of a convention. (c) **Absorption**: the
   explorer becomes a picker over a path-completion source and the picker
   becomes a one-field form with a filtered list, leaving two surfaces total. I
   recommend (b), taking (c) for the explorer only if it falls out cheaply. This
   is the largest sizing decision in the document — (c) roughly doubles the
   phase.
6. **The reading overlay.** Confirming R1 Q3: a `MODAL` surface, full content
   height minus a margin, width `min(content_width, MAX)` centered, with `MAX`
   around 80–100 columns. Is `MAX` a config key under `[ui.pager]` or a
   constant? And on close, does it return to the preview with the index
   selection unchanged (so the overlay is purely a zoom), or does it keep its
   own cursor so `J`/`K` inside the overlay can walk messages without moving the
   index?
7. **Contacts under the unified layout.** §1.3 wants contacts on the same
   machinery, but it is list-and-detail — two panes, not three. Does it become
   the same column system with n=2, sharing collapse rules and focus cycling? Or
   does it grow a third column (vdir has collections, though nitidus exposes no
   contact-folder concept today) purely for symmetry with the mail tab? I
   recommend n=2 and no invented third pane.
8. **The shape of unified focus.** Phase 1, per R1 Q2. Proposal: one app-level
   `Focus` resource naming the focused pane within the active tab, replacing
   `SidebarState.focused` and `ContactsView.focus`, with the keymap context
   derived from it rather than from `Screen`. That makes `CONTEXT_PAGER` mean
   "the reading pane has focus" and leaves `Screen` owning only Compose versus
   the tabbed surfaces — or possibly nothing at all. Does `Screen` survive R2 in
   any form, or does the tab plus focus pair replace it outright?

### 3.4 R2 Answers

1. I see. In that case I think we should remove peek as a function completely,
   and make any read action (whether it's been selected in the preview pane (3rd
   column pane), or been opened in the full reader overlay). Does that make
   sense?
2. So to iterate, moving the arrow down to a new message in the index does not
   load the message in the preview pane. So we will need to track what message
   is currently in the preview pane separately from current index value, if that
   is not already the case. Message fetching should only happen when (a) an
   index is selected via "enter" or "right arrow", or (b) opened in the full
   reader overlay via some hotkey. We may need to add some loading indicator for
   the pane and overlay while the message loads. Concerns? Recommendations?
3. Let's have it slide up as an overlay, but keep it in the bottom right
   quadrant of the screen.
4. bottom status line should keep current info, and version. Also remember that
   statusline also is where commands are typed and chosen.
5. I also prefer (b)
6. Let's make max a config item. On close it should return to index, no walker
   as that's what the preview pane is for. My guess is that 99% of the time
   people will just use the preview pane as they are using the app on a large
   screen.
7. Actually let's defer contacts for after this is done. If changes in this
   refactor will affect contacts layout, then let me know and I'll come up with
   a stopgap plan.
8. replace completely, compose can be folded into an overlay or preview pane.
   Perhaps replies can be done in preview pane with index still selected on the
   replied message, and new composed messages can be created in an overlay,
   which will also use the form overlay for to, subject, etc. Any issues with
   that from a technical aspect?

### 3.5 R3 Questions

R2 settled a great deal, and A2 in particular made the feature _smaller_: an
explicitly-loaded preview needs no debounce, no stale-fetch cancellation, and
arguably no body cache. What remains is mostly consequences.

1. **peek removal versus `mark_read = "never"`.** Agreed on deleting the delay —
   once loading is explicit, `After(delay)` has nothing left to defer, and
   `peek.rs` plus `PeekTimer` go with it. But `MarkRead` has a second axis:
   `Never` is not a timing preference, it is an opt-out from auto-marking at
   all, and someone triaging a mailbox may genuinely want to open messages
   without flagging them. Proposal: delete `After`, keep
   `mark_read = "open" | "never"`, and let "open" mean "whenever a fetch
   completes into the preview pane or the reading overlay". Two loose ends if
   you agree: an existing `config.toml` carrying `mark_read = 2.0` needs a
   verdict (hard error, or coerce to `"open"` with a startup notice — I lean
   coerce), and this is a **specification change** — `specification.md:49` and
   `roadmap.md:27` both list mark-read delay as a shipped feature, unlike
   overlay-forms which needed no spec edit.
2. **The preview goes stale, and that needs a stated answer.** This is the
   biggest thing A2 implies. If the preview loads only on `Enter`/`→`, then
   arrowing the index leaves the preview showing a _different_ message than the
   selected row — which is the correct trade for the network, but the user has
   to be able to see it. Options: (a) the preview keeps the loaded message and
   the index marks which row it belongs to, with a gutter marker or row style;
   (b) the preview clears when the selection moves off the loaded message; (c)
   the preview keeps the content but dims it. I recommend (a) — it is the only
   one where the pane stays useful for "read this while I scan the list", and
   feature-index-custom-v1 already gave the index themed per-row styling to
   carry the marker. Related and smaller: what does the preview show before
   anything has been loaded, and after the loaded message is deleted or moved?
3. **Scope reduction to confirm, and the cache.** Three things I flagged as work
   in R2 Q2 largely evaporate under A2. Debounce: not needed. Stale-fetch
   cancellation: already handled — `PagerState` drops a response whose job is
   not the current one. Loading indicator: partly exists — `pager/render.rs`
   already renders `loading…` when a fetch is outstanding, so the question is
   only whether the pane and overlay want something better than a text line.
   That leaves the body cache, which was load-bearing only under a following
   preview and is now merely an optimization for re-reading a message. Recommend
   dropping it from this document entirely and recording it as a `nitidus-mail`
   follow-up. Agreed, or do you want the in-memory LRU anyway?
4. **Statusline position/total.** A4 settles what the row keeps (current info,
   version, the `:` command line and `/` search) but not the question underneath
   R2 Q4: with three panes visible, does the position/total segment follow the
   focused pane — folder n/m with the sidebar focused, message n/m with the
   index focused — or stay index-owned no matter what has focus?
5. **Contacts cannot be fully deferred — flagging as A7 asked.** Its _layout_
   can, but three other decisions reach it regardless. `Screen::Contacts`
   disappears when `Screen` does (A8). `ContactsView.focus` is one of the two
   mechanisms unified focus exists to replace, so phase 1 (R1 A2) touches it by
   definition. And nine of the twenty prompt call sites live in
   `contacts/{add,edit,mutate,transfer}.rs`, which R1 A1 put in scope. Proposed
   split: contacts is ported to unified focus, `Screen` removal and overlay
   forms in this document, but keeps its own local `TABLE_PANE_PERCENT` split
   rather than moving onto the shared column system — that migration becomes
   refactor-ui-v2. Acceptable, or would you rather contacts move onto the shared
   layout here too, since it is already being opened?
6. **Compose as an overlay or pane — four real issues (A8 asked).**
   1. **Body editing needs width.** `compose/inline.rs` is a ratatui-textarea
      session under `InputMode::Editor`. In a preview pane at a third of an
      80-column terminal that is roughly 26 columns, which is not a usable
      composition width. Reply-in-preview therefore needs the same zoom-to-
      overlay escape reading has, or a minimum-width rule that forces compose
      into the overlay on narrow terminals. Your instinct in A6 — that most
      people are on a large screen — is probably right, but the narrow case
      needs a defined behavior rather than a bad one.
   2. **Where the body lives if the headers are a form.** Either the compose
      overlay is one surface with a header-form region above a body editor, or
      the header form closes and hands off to a body-editing pane. I strongly
      recommend the first: the second means Cancel has two different meanings
      depending on which half you are in, and postpone/recall would have to
      capture a half-filled state.
   3. **The review screen.** `compose/render.rs` is 364 lines of full-screen
      review. The stepped form already supports pages, so review can become the
      final page of the compose overlay — but that only works if the body editor
      is inside the same surface, i.e. option 1 above. Otherwise review needs to
      stay its own thing.
   4. **`refuse_while_composing`.** `shell.rs` currently blocks tab switching
      while composing, on the grounds that tabbing away would orphan the
      session. If compose becomes a pane, that guard either goes — drafts
      already survive via postpone/recall, so nothing is actually lost — or it
      stays and makes the pane pseudo-modal, which is worse than the screen it
      replaced. Recommend dropping the guard.
7. **Specification and roadmap edits.** Beyond the mark-read entry in Q1,
   `specification.md` describes the pager, the composer flow, and the statusline
   in terms this refactor changes. Should the plan carry a phase that updates
   `specification.md` and `roadmap.md`, or do you want to make those edits
   yourself once the shape is final?

### 3.6 R3 Answers

1. agreed, coerce, and change spec.
2. (a), and show a short help message with nav/enter instructions.
3. agreed, drop.
4. Let's keep it index-owned.
5. acceptable
6. -
   1. let's have a minimum width rule
   2. first
   3. yep
   4. drop
7. yes, please update

## 4. Plan

Ten phases, each leaving the workspace compiling and the suite green, each its
own commit. The ordering is dependency-driven: phases 1 and 2 are the tier 0
mechanisms everything else consumes, and no phase deletes a mechanism before its
last consumer has moved off it.

This is a large branch — larger than any single feature so far. Phases 1–4 are
self-contained and leave the app in a coherent state with no half-migrated
surfaces; if the branch needs splitting, that is the seam.

### Phase 1 — Unified pane focus

Tier 0, and the prerequisite R1 A2 named. No visible behavior change.

- `Focus` resource and the `Pane` enum (§2.1); a tab remembers its own focus.
- Replace `SidebarState.focused` and `ContactsView.focus`; both types keep their
  other state.
- Derive the keymap context from `Focus` instead of `Screen`. `Screen` survives
  this phase, now only distinguishing Compose from the tabbed surfaces.
- Tests: focus round-trips per tab; the context resolves from focus for every
  pane; existing sidebar and contacts navigation tests pass untouched.

### Phase 2 — One overlay surface

Tier 0. No visible behavior change.

- `OverlaySurface` with an explicit stack and a single router gate, replacing
  the nine sequential gates.
- Migrate picker, form, explorer and the attach preview onto it; the explorer
  gains a rebindable context.
- Shared chrome: frame, title, focus ring, mouse, `layer` rung.
- Tests: push/pop ordering; the top of the stack takes the keyboard; a global
  binding does not leak through any surface; the explorer's new context resolves.

### Phase 3 — Confirmations

- `Confirm` surface on the form machinery at `layer::MODAL`, safe option
  focused, `Esc` cancels.
- Migrate the seven sites in §1.2 tier 4.
- Tests: each of the seven confirms and declines correctly; `Enter` on first
  open never destroys; the existing delete/discard/remove-account assertions
  survive the move.

### Phase 4 — Toasts and the message log

- `StatusMessage` becomes a bounded ring buffer; severity policy per §2.6.
- Message log surface, bottom-right quadrant, on the shared surface of phase 2.
- Migrate the 76 status references to the policy; the statusline keeps `Info`.
- Tests: severity routes to the right destination; the buffer bounds and evicts
  oldest-first; the log renders what was written; `Info` still reaches the
  statusline.

### Phase 5 — The column budget

- n-column layout in `ui-kit` with preferred/minimum widths and priority-ordered
  collapse; `sidebar_split` retired.
- Mail tab moves onto it at two columns — the reading pane does not exist yet,
  so this is a pure layout swap.
- Tests: collapse order drops reading before folders; a terminal too narrow for
  any budget still produces non-overlapping rects; the existing sidebar split
  cases hold under the new machinery.

### Phase 6 — The reading pane

- Third column; its own loaded-message state, independent of the index cursor.
- Explicit load on `Enter` / `→`; the index marks the loaded row; empty-state
  help line.
- Tests: arrowing the index does not fetch; `Enter` fetches exactly once; the
  loaded marker tracks the pane and not the cursor; the pane survives the
  loaded message being deleted or moved.

### Phase 7 — The reading overlay, and peek's deletion

- Reading overlay at `layer::MODAL`, `[ui.pager]` max-width config key, close
  returns to the list with the selection unchanged.
- Delete `peek.rs`, `PeekTimer`, `MarkRead::After`; `mark_read` becomes
  `"open" | "never"`; a numeric value in an existing config coerces to `"open"`
  with a startup notice.
- `Screen::Pager` is removed.
- Tests: `"open"` flags on fetch completion in both pane and overlay; `"never"`
  flags in neither; a numeric config coerces and notices; the overlay collapses
  to the pane's selection on close.

### Phase 8 — The remaining prompts

- Migrate the thirteen data-entry sites to forms, collapsing the chained ones
  (contacts add, contacts edit) into multi-field forms the way the wizard's
  thirteen steps collapsed.
- Delete `prompt/mod.rs`, `prompt/panel.rs`, `InputMode::Prompt` and its gate.
- Tests: each migrated site round-trips its value; the contact chains validate
  per field; nothing references `InputMode::Prompt`.

### Phase 9 — Compose as an overlay

- Compose overlay holding headers, body and review as pages of one stepped form.
- Reply opens in the reading pane; new composition opens in the overlay; below
  `MIN_PANE_WIDTH` a reply goes to the overlay too.
- Drop `refuse_while_composing`.
- `Screen::Compose` is removed, and with it `Screen` and `MailScreenMemory`.
- Tests: reply lands in the pane with the index unmoved; a narrow terminal
  forces the overlay; postpone and recall survive a tab switch mid-composition;
  the review page still gates sending.

### Phase 10 — Documentation, verification, cleanup

- Update `specification.md` (mark-read delay, the pager, the composer flow, the
  statusline) and `roadmap.md` item 27, per R3 A7.
- `cargo clippy --workspace --all-targets` clean, `cargo fmt --all --check`
  clean, `CARGO_INCREMENTAL=0 cargo test --workspace` green with pass counts
  recorded in §5.
- Run the cleanup skill over the new modules; fill in §§5–7.

## 5. Verification

Measured at the branch point (`b613c7a`, the overlay-forms merge) and again
after phase 10:

| Command                                  | Before  | After   |
| ---------------------------------------- | ------- | ------- |
| `cargo test --workspace` (passed/failed) | 545 / 0 | 611 / 0 |
| `cargo clippy --workspace --all-targets` | clean   | clean   |
| `cargo fmt --all --check`                | clean   | clean   |

Test runs used `CARGO_INCREMENTAL=0`, per `rules/testing.md`. Sixty-six net new
tests across ten phases, each verified green before the next began, so no phase
left the workspace broken.

This is a refactor by name but not by contract: §2 is a list of intended
behaviour changes. What is preserved is every *capability* — nothing the app
could do before it cannot do now — and the migrated tests keep their original
assertions wherever the behaviour they pinned survived. Where an assertion
pinned a mechanism rather than a behaviour (prompt label text, `Screen`
variants, the mark-read delay), it was rewritten against the replacement and
the change is called out in §7.

## 6. Implementation Report

### What landed

Ten phases, in order, one commit each except phase 8 which took two:

1. `focus::PaneFocus` — one focused pane per tab, replacing the sidebar's
   `focused` bool and the contact book's own `PaneFocus`.
2. `overlay::surface::OverlayStack` — one router gate and an explicit stack for
   every modal; the explorer gained a rebindable context.
3. `overlay/confirm/` — the seven y/n questions as modal surfaces.
4. `MessageLog` — severity routing into toasts, with `:messages` behind it.
5. `ui-kit::layout::split_columns` — an n-column budget with collapse rules.
6. The reading pane as a third column.
7. The zoomed reading overlay; peek deleted.
8. Form field completion, then the last fourteen prompts migrated and `prompt/`
   deleted.
9. The composer stopped being a screen; `Screen` deleted.
10. Documentation, verification, cleanup.

### Four findings that changed the design

**peek was never a preview.** §1.4 records it: `peek.rs` was the deferred-SEEN
timer, not a partial load, and the app already fetched only on `Enter`. The
document's opening premise — that a following preview needed debounce and
stale-fetch cancellation — was answering a problem the code did not have. R2 A2
made loading explicit by design rather than by accident, and the debounce work
disappeared with it.

**`Screen` was a third copy of state two other resources already held.** Which
tab is active is `Tabs`; whether a composition is open is `ComposeState`. Every
compose entry point carried a line keeping `Screen` in sync, and the help
overlay's copy of the context rule had already drifted from the router's. One
`focus::active_context` replaced both.

**A raised pane is not a modal.** §2.3 put the reading overlay at `layer::MODAL`.
`:links` opens a picker from the reading pane, and at `MODAL` the zoomed pane
would have covered the picker it spawned. `layer::ZOOM` sits between `BASE` and
`PANEL` instead: it draws over its neighbours and under everything that takes
the keyboard. The composer uses the same rung for the same reason.

**Forms needed completion before compose could leave the prompt.** The address
headers were the one prompt using candidate cycling, which
feature-overlay-forms had explicitly deferred until a form needed it. Tab
belongs to focus on a multi-field surface, so cycling took `C-n`/`C-p` rather
than overloading Tab the way a single-field prompt could.

### Deviations from §2, and why

- **Confirmations are their own surface, not a `FormSpec`** (§2.5 said the form
  machinery). `FormState` is built around pages, fields and an id-keyed value
  map; a confirmation has none of those. Fitting it would have meant four
  structural changes to the form to serve a surface with no fields. What is
  shared is what actually repeats: `draw_frame`, and the `Interaction`/`Visual`
  vocabulary promoted out of `form/` into `overlay::interaction`.
- **The message log is not built on the picker** (§2.10 assumed it was). A log
  has no selection callback, wants per-row severity styling, and needs a corner
  layout the picker hardcodes as centered. Bending the picker to serve one
  consumer would have added two features to it.
- **Pane gutters were not in the design at all.** Neighbouring columns abutted
  with nothing to separate them. The budget now reserves a one-column rule per
  seam, recorded in §2.2.

### Not done

**The composer is one surface but not one form.** R3 A6.2 and A6.3 asked for
headers, body and review as pages of one stepped form. The headers are a form
and the session is one unit — Cancel means one thing, postpone captures whole
state — but the body and review still draw in the compose surface rather than
as form pages. Reaching the answered design needs `FieldKind::Multiline`:
variable-height field geometry, and the form delegating keys to the editor when
a multiline field has focus. That is a form-subsystem change rather than a
migration, and half of it would be worse than none.

**Contacts keeps its own layout**, per R3 A5. It was ported to unified focus,
`Screen` removal and overlay forms; `contacts/draw.rs` still splits its content
region with a local `TABLE_PANE_PERCENT` rather than the column budget.

### Follow-ups

- A message body cache (R3 A3), as a `nitidus-mail` change.
- `FieldSpan::Half`, still unbuilt from feature-overlay-forms; the contact
  component forms are the strongest case for it yet.
- The statusline's position/total still follows the index regardless of which
  pane has focus (R3 A4), which is the answered behaviour but reads oddly with
  the folder tree focused.

## 7. Testing and Cleanup

### Tests

Sixty-six net new, weighted toward behaviour. The larger groups:

- **Focus and context** (5) — focus round-trips per tab, and a focused mail pane
  cannot claim the sidebar context while the contact book is on screen. That
  last one pins a leak the old global flag had to be cleared to avoid.
- **The overlay stack** (6) — push/pop ordering, a surface that closes without
  popping not stranding the keyboard, the explorer's context being rebindable,
  and global bindings not leaking through it.
- **Confirmations** (10) — geometry, and the property the surface exists for:
  `Enter` on a freshly-opened confirmation declines rather than destroying.
- **Feedback** (16) — severity routing, the ring buffer evicting oldest-first,
  the log panel windowing and clamping its scrollback.
- **The column budget** (13) — collapse order, gutters, and non-overlapping
  rects at every width from 0 to 140.
- **The reading pane** (5) — arrowing the index neither fetches nor disturbs the
  pane, the zoomed pane clearing what is underneath, and a picker still drawing
  above it.
- **Form completion** (4) — cycling without stealing Tab, and rewriting only the
  address being typed.

Two tests were verified to fail against the pre-change code rather than trusted
green: the context-leak test, and the zoom-bleed test.

### Assertions that changed rather than moved

Called out because they are the places behaviour, not mechanism, was pinned:

- Prompt-label assertions became form-value assertions. Where a test matched
  prompt text verbatim (`"Delete 2 permanently? (y/n): "`), it now asserts the
  count reaches the question, which is what the test was about.
- `Screen` assertions became `Tabs::is_contacts` or `ComposeState::is_active`.
  Several had conflated "the mail tab" with "not composing"; each was repointed
  at the one it meant.
- The two peek-delay tests were replaced by mark-read policy tests, the delay
  having been deleted.
- `composing_refuses_tab_switches_with_a_notice` became
  `tabbing_away_mid_composition_is_allowed`, inverted deliberately with the
  guard it tested.

### Cleanup

Ran the cleanup skill over the eight modules this branch added. Clippy and the
compiler flagged nothing, so removal was grep-driven:

- `PaneFocus::contacts()` had no callers — a leftover from before the contact
  book read focus through `is_focused`.
- `focus::mail_context` was public but only `active_context` calls it; narrowed.
- `LogEntry.at_secs` was written on every message and never read. Dropped by
  agreement rather than earned by showing timestamps in the panel.

Three comments reworded: `overlay::interaction` still said "the form" after the
confirmation surface started sharing it, `status` cited a call-site count that
would drift, and `pager/mod.rs` referenced `Screen::Pager` after its deletion.
