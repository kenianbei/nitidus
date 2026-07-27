# feature - Compose as a Form - v1

The composer becomes one form. From, To, Cc, Bcc, Subject and the body are
tab-focusable fields on a single surface, whether that surface is the reading
column beside a message or the overlay over the panes. Today those six things
live in three different input regimes — a read-only review screen, per-header
modal forms, and an editor mode — and the only way between them is a
single-letter binding.

Filed as a **feature** rather than a refactor: it changes what the composer
does, not only how it is built. §3 q1 asks whether that reading is right.

## 1. Current Design

### One widget, three input regimes

`compose/render.rs` spawns a single `ComposeWidget` at `layer::ZOOM` whose
layout closure re-decides placement on every resize (`compose_layout`): a reply
takes the reading column (`mail_layout(MailPane::Reading, …)`), a new message —
or a reply in a column below `MIN_PANE_WIDTH` — takes `centered_capped` over the
panes. Both placements draw the same thing: a `Paragraph` of `header_lines` +
body, plus a two-row cheat sheet generated from the live keymap.

Nothing on that surface is focusable. Editing reaches it three ways:

- **Headers** — `ComposeOp::{To, Cc, Bcc, Subject}` (`t` `c` `b` `s`) call
  `prompt_header`, which opens a one-field modal `FormSpec` with the id
  `"value"`, prefilled from the session, whose submit writes the field back.
  Opening a new message first runs `open_headers_form` — a two-field form (To,
  Subject) — then falls straight into the body editor.
- **Body** — `ComposeOp::EditBody` (`e`) enters `InlineEditor`: a
  `TextArea<'static>` behind an `Arc<Mutex<…>>`, `InputMode::Editor` on the
  router, and the 26-binding `CONTEXT_EDITOR` keymap. While it is open,
  `render_editor` pins `header_lines` above the text area inside the same
  widget. `ComposeOp::EditBodyExternal` (`E`) suspends to `$EDITOR` instead.
- **From** — not editable anywhere. `ComposeSession::create` derives it once
  from the account config (`from_identity`) into `session.from`, and
  `session.account` picks the transport. The signature is baked into the body
  file at creation, so From and the body are only coupled at that instant.

Which keymap is live is decided by `focus::active_context`: an active
`ComposeState` returns `CONTEXT_COMPOSE` wherever the composer is drawn, ahead
of tab and pane focus. `router::route_key` gates in a fixed order — command
line, search, `overlay::surface::route_key`, `InputMode::Editor`, then the
context lookup — so the three regimes never see each other's keys.

### The form machinery it would have to run on

`overlay/form/` (nine files) is declarative: `FormSpec` + `FieldSpec` describe,
`state.rs` behaves, `render.rs` draws.

- `FieldKind` is `Text { masked }` or `Select { options }`. Text fields edit
  through tui-prompts `TextState` — single-line by construction.
- `FormState` owns the derived page list, one `FormValues` map keyed by field
  id, `Focus::Field(usize) | Focus::Button(usize)`, validation, and a
  `generation` counter that respawns the control set when its shape changes. Tab
  order is fields then buttons, wrapping both ways (`move_focus`).
- `geometry::form_geometry` centers a panel at `PANEL_WIDTH_PCT = 60` and gives
  **one row per field**, `LABEL_WIDTH = 18` for the label column, then a message
  row and a right-aligned button row. Buttons are
  `ButtonRole::{Cancel, Back, Primary}` only.
- `entity.rs` spawns one entity per control at `layer::OVERLAY + 1` carrying
  `UiFocusable`, `UiHoverable`, and mouse passthrough, with each control's
  `WidgetLayout` calling back into `form_geometry` — so a click can never land
  where the drawing did not.
- `ActiveForm` is a **singleton** resource, and `open_form` calls
  `surface::raise(Surface::Form)`. `surface::route_key` hands the top surface
  _every_ key.

### What this costs today

- From cannot be changed after the first keystroke of a composition.
- Reaching Cc means remembering `c`; an empty Cc renders as `(none)` and is not
  a place you can go.
- The opening chain still asks two questions before showing the message, and
  `t`/`s` re-ask them one at a time afterwards.
- Body editing is a mode with its own keymap, its own cheat sheet, and its own
  way out (`Esc` → `:editor-done`).

### Facts that constrain the change

- One row per field and a centered panel are baked into `form_geometry`; there
  is no host rect and no multi-row field.
- `ActiveForm` holds one form. A compose form would occupy it while the attach
  path prompt, the discard confirm, and the detach picker still need to open
  _over_ the composition.
- `Surface::Form` on the overlay stack consumes every key. A form drawn in the
  reading column is not modal in the same sense.
- `<Tab>` is `:form-focus-next` in `CONTEXT_FORM`; the body field must not
  swallow it.
- `persist::persist_session` and the send/postpone paths read `ComposeSession`,
  not `FormValues`.
- Tests that drive the current flow: `tests/compose.rs` (5), `tests/drafts.rs`
  (4), `tests/reply.rs` (7), `tests/inline_editor.rs` (21), `tests/outbox.rs`
  (4), plus `overlay/form/tests.rs` (768 lines) and `state_tests.rs`.

## 2. Proposal

One `FormSpec` per compose session, opened when the session starts and living as
long as it does, with six fields: From, To, Cc, Bcc, Subject, Body. Tab and
Shift-Tab walk them; the completion, validation, mouse, and focus machinery that
`overlay/form/` already has applies to all six without a second implementation.

Four pieces of new machinery, each small and each useful beyond compose:

**2.1 A body field kind.** `FieldKind::Body` backed by `ratatui-textarea`,
reusing what `compose/inline.rs` already knows about the shared `TextArea`, line
styling, and attachment tokens. Its runtime holds the `SharedArea` instead of a
`TextState`. `FieldRuntime::edit` currently refuses only Enter and Esc; a body
field wants Enter and refuses Tab.

**2.2 Variable-height rows.** `FormMetrics` carries a row count per field rather
than a single `field_count`, and the body field claims what is left after the
chrome. `form_geometry` stays the one place that decides where anything lands,
so hit-testing follows for free.

**2.3 Placement.** A `FormPlacement` on the spec — `Overlay` (today's centered
panel at `layer::OVERLAY`) or `Host { layout, order }` — lets the compose form
reuse `compose_layout` verbatim and draw in the reading column at `layer::ZOOM`
for a reply, over the panes for a new message, re-deciding on resize exactly as
it does now.

**2.4 Editor bindings under form bindings.** While a body field has focus, keys
resolve against `CONTEXT_EDITOR` layered under `CONTEXT_FORM`, so the 26
existing editor bindings keep working, `<Tab>` still means focus, and
`:editor-done` becomes "focus the next control" rather than "leave a mode".
`InputMode::Editor` and the separate editor cheat sheet disappear.

What falls out: `ComposeStage::{Prompting, Editing, Review}` collapses,
`open_headers_form` and `prompt_header` are deleted,
`ComposeOp::{To, Cc, Bcc, Subject}` and their `:compose-to`/`:compose-cc`/…
commands lose their reason to exist, and the cheat sheet describes one context
instead of two.

What does not change: `$EDITOR` stays as `:compose-edit-external`, the send
pipeline, drafts, outbox, threading headers, attachment tokens, and crash
recovery all keep reading `ComposeSession`.

The parts this proposal deliberately does not settle — what happens to `From`,
where Send and Postpone live once single letters type into fields, and how a
prompt opens over a form — are §3.

## 3. Discussion

### 3.1 R1 Questions

1. **Type.** Filed as a feature because the composer's behaviour changes
   (Tab-navigable headers, an editable From, no review stage, single-letter
   compose bindings retired). Is that the right classification, or do you want
   it framed as a refactor of the compose surface with the behaviour changes
   called out as exceptions?

2. **Does the review stage survive?** With every field editable in place, is
   there still a distinct "review before send" state — one form with focus
   parked on the Send button — or does `ComposeStage` collapse to nothing and
   the form simply exist while the session does?

3. **Where do Send, Postpone, Attach, Detach and Discard go?** Once printable
   keys type into the focused field, `y`/`p`/`a`/`d` cannot stay. Options:
   buttons on the form (which needs more than
   `ButtonRole::{Cancel, Back, Primary}`), chords in the form context
   (`Ctrl-Enter` send, `Ctrl-p` postpone), the `:` commands only, or some
   combination. Which of the five deserve a button, and what should the primary
   button say?

4. **From.** A select over the configured accounts' identities, or a free-text
   field, or focusable-but-read-only? If it can change mid-composition: the
   signature is already baked into the body file, and `session.account` drives
   the transport and the Sent folder — should switching From rewrite the
   signature, and is it refused once a reply's threading headers are set?

5. **Body field and the editor.** Confirm the intent: `<Tab>` always leaves the
   body (never inserts a tab), `<Enter>` always inserts a newline, and the
   `editor` keymap stays live while the body has focus. Should `:compose-edit`
   survive as a "zoom the body to full height" toggle, or is the body just
   another field that happens to be tall?

6. **Modality in the reading column.** When a reply's form is drawn in the
   reading column, does it own the keyboard as compose does today, or do the
   index and sidebar stay live so `j`/`k` still move the message list under it?
   The answer decides whether the compose form goes on the overlay stack at all.

7. **Forms over forms.** The attach path prompt, the discard confirm, the detach
   picker and the send-check confirms all open while a composition is open.
   `ActiveForm` holds exactly one form. Should it become a stack, should compose
   live in its own resource outside `ActiveForm`, or should those prompts move
   off the form surface?

8. **Narrow columns.** Six rows of chrome plus an 18-column label gutter is a
   lot for a reading column on an 80-column terminal. Should empty Cc/Bcc
   collapse into a single "add Cc/Bcc" affordance below some width, should the
   label column shrink, or should all six rows always show?

9. **Attachments.** They currently appear as `Attach:` rows in the header block
   _and_ as tokens in the body. In a form, do they stay a read-only block
   between Subject and the body, become a focusable list, or live only as body
   tokens?

10. **Validation and the send checks.** `send_with_checks` warns about an empty
    subject and a forgotten attachment through a confirm chain. Do those become
    field validators (which would block Tab-out or the primary button), or stay
    as confirms on the way out?

11. **Source of truth.** Does the form write through to `ComposeSession` on
    every change — keeping `persist_session`, postpone and crash recovery
    reading one shape — or does `FormValues` own the headers until submit?

### 3.2 R1 Answers

1. feature
2. no need for review before send.
3. Since the form needs to be usable in both pane and overlay, we should only
   provide send and cancel as buttons. We should also make use of cursor
   position in the textarea to allow inline attachments to be automated. For
   example require the attachment to be added via hotkey or command, and then
   another hotkey to insert into current cursor. This could also be a
   ctrl-{char} that can be used while textarea is focused. I like the idea of
   using ctrl hotkeys or alt hotkeys to do composer related commands that can be
   run from the textarea as well as other text prompts.
4. focusable but readonly for now
5. yes, tab always leaves, enter for newline, and consider allowing ctrl or alt
   hotkeys for any commands.
6. it owns the keyboard
7. no form over forms. please explain the rest of this question in more detail.
8. all should show
9. focusable horizontal list if possible. also let's require attachments to be
   attached first before being set as inline attachments.
10. field validators
11. let's write though.

Also, let's move the To autocomplete to appear below the to field if possible.

### 3.3 R2 Questions

**Answer 7, in detail.** `ActiveForm` is `Option<FormState>` and `open_form`
overwrites whatever is in it. Compose is not a form today, so nothing collides.
Once compose _is_ a form that stays open for the whole session, anything that
opens a form during a composition overwrites it — losing every header, the body
runtime, and the write-through target.

Auditing what a composition can open today:

- `:attach` → `drafts::attach_prompt` opens **a form** (one validated `File`
  field). This is the only real collision.
- `:detach` → a picker (`Surface::Picker`), its own resource — stacks fine.
- `:discard` → a confirm (`Surface::Confirm`) — fine.
- The send checks → up to two chained confirms — fine.

So "no forms over forms" costs exactly one thing: the attach prompt stops being
a form. Three ways out:

- **(a) Attach through the explorer.** `crate::explorer` already is a modal file
  browser with an `on_pick` callback, an extension filter and its own surface.
  `Alt-a` opens it; picking adds the file to the attachment list. No second
  form, no new machinery, and a browser is the better affordance for choosing a
  file than a path field. `:attach <path>` stays for the typed case.
- **(b) Compose gets its own resource.** A `ComposeForm` holding a `FormState`,
  leaving `ActiveForm` free for transient forms. `FormState` is self-contained,
  so this is mostly plumbing — but `entity.rs`, `panel.rs`, `interaction.rs` and
  `mouse.rs` all query `ActiveForm` by name and would each have to serve two.
- **(c) A form stack** — ruled out by answer 7.

I recommend (a), which leaves the invariant "at most one form exists, and while
composing it is the composer" true and needs no new plumbing.

1. **Attach.** Confirm (a) — attach opens the explorer, and the compose form
   stays the single occupant of `ActiveForm`? Or do you want (b) as insurance
   anyway?

2. **Enter on a header field.** Today Enter with a field focused fires the
   page's primary action (`activate`: `Focus::Field(_) => ButtonRole::Primary`).
   With Send as the primary button, a stray Enter in Subject sends the message.
   Options: Enter moves to the next field and Send needs the button or its
   hotkey; Enter fires Send only when a button has focus; or keep today's rule.

3. **Esc, and what Cancel means.** Esc is `:form-cancel`, which in the composer
   currently means "discard, after a confirm". With only Send and Cancel as
   buttons: is Cancel _discard_ (confirm, then delete the body file) or
   _postpone_ (save to Drafts and close)? If it is discard, postpone is
   hotkey-only. A button labelled "Cancel" that throws a message away also reads
   wrong — "Discard" or "Postpone" instead?

4. **The hotkey modifier.** Ctrl is crowded: the editor context binds
   `Ctrl-z/y/s/a/x/w/v/k/p` plus the Ctrl-arrows, and the form context binds
   `Ctrl-n`/`Ctrl-p` for completion. Alt is empty in every context. Proposed
   set, live from any field:

   | key         | command                               |
   | ----------- | ------------------------------------- |
   | `Alt-Enter` | `:send`                               |
   | `Alt-p`     | `:postpone`                           |
   | `Alt-a`     | `:attach` (explorer)                  |
   | `Alt-i`     | insert the selected attachment inline |
   | `Alt-d`     | `:detach`                             |
   | `Alt-e`     | `:compose-edit-external`              |
   | `Alt-x`     | `:discard`                            |

   Corrections? And is `Alt-Enter` right for send, or should sending be harder
   to hit by accident?

5. **Where the hotkeys live.** "Commands that can be run from the textarea as
   well as other text prompts" reads like a layer rather than a context: the
   `compose` context resolved _beneath_ whichever context owns the focused
   control, so the same key answers from a header field, the body, and the
   attachment list, and stays rebindable in `keys.toml`.
   `Keymaps::resolve_layered` already does this for `global`. Confirm: keep the
   name `compose`, layer it under `form` and `editor`, and generalize
   `resolve_layered` to take a stack rather than one context plus global?

6. **How far the attachment model inverts.** Today the body's tokens are the
   source of truth: `sync_attachments` derives `session.attachments` from
   `token::paths(&session.body)`, and `build` strips the token lines and
   attaches every derived path. "Attached first, then inserted" makes the list
   authoritative and a token a _placement_ of something already attached. Three
   consequences to settle:
   - An attached file with no token in the body: ordinary attachment on the wire
     (my assumption), or an incomplete state?
   - Does a token mean a real inline part — `Content-Disposition: inline` with a
     `cid:` reference inside `multipart/related`, which is new work in
     `build.rs` — or is the token a position marker in v1 while the MIME stays a
     plain attachment?
   - Deleting a token line detaches the file today. With the list authoritative
     it should only remove the placement and leave the file attached. Confirm?

7. **The attachment list's shape.** One tab stop for the whole row with
   Left/Right moving between attachments (like a select), or one stop per
   attachment? What do Enter and Delete do on a focused attachment — preview
   (`compose/preview.rs` exists) and detach? And with nothing attached, does the
   row vanish — changing the tab-stop count as you attach — or stay as an empty
   row?

8. **Completion below the field.** The panel is bottom-anchored today
   (`layout::bottom_panel_layout`, above the statusline) and shared with the
   command line. Anchoring it under the focused field needs that field's rect,
   which `form_geometry` has. Two edge cases: with no room below — a field near
   the bottom of a short reading column — flip above the field, or clamp? And
   does this become the rule for _every_ completed form field, with the command
   line keeping the bottom panel?

9. **What the validators enforce.** A field validator blocks the primary button
   and takes focus on failure. Mapping today's rules onto that:
   - To: non-empty and parseable? Today an empty To only fails at build time,
     and `postpone_unaddressed` deliberately tolerates it. Should postpone skip
     validation entirely so an unaddressed draft still saves?
   - Subject: empty is a warning you can override today. As a validator it
     becomes a hard block. Is that intended, or does the form need a _warning_
     severity that Send can pass through after a confirm?
   - Forgotten attachment: body-derived, not a field rule. A validator on the
     body field, or does it stay a send-time confirm?

10. **Read-only From.** Confirm it takes a tab stop and simply refuses edits
    (dimmed, no cursor) rather than being skipped by Tab, and that switching
    accounts mid-composition is a v2 concern.

11. **Write-through cadence.** Headers into `ComposeSession` on every keystroke
    is cheap. The body is the expensive one: `session.body`, the crash-survival
    file, and the sidecar. Today `persist_session` rewrites the sidecar on
    session change, and the body file is written when the editor closes. With no
    editor to close, should the body file follow the sidecar's cadence, or
    throttle — at most every N seconds, plus on focus-out and on send?

### 3.4 R2 Answers

1. (a) confirmed
2. option 1
3. Discard, for postpone it can continue to be a command/hotkey
4. all the alt keys look good
5. confirm
6. confirm
7. yes, tab then left/right. yes, enter/delete as proposed. stay as an empty row
   with Add Attachment button, when focused and enter opens picker
8. clamp. yes.
9. good point...let's keep validation only at build time then.
10. yes
11. yes, as proposed (throttle)

### 3.5 R3 Feedback

> One issue with how hotkeys are presented on the bottom of the compose form,
> they are always cropped as there are too many of them. Let's remove
> help/hotkeys from being displayed inline, and use yazi's strategy of `~` or
> `F1` to show the help overlay.

The border hint was a decision taken during the work, not one this document
asked for — see §6. It does not survive contact: seven Alt commands spelled out
run past any frame the composer is drawn in, and the reading column is the
narrow case by design.

### 3.6 R3 Resolution

Three points needed settling before the change was worth making:

1. **`~` cannot be bound inside the form.** Yazi's main view has no text entry;
   the composer is nothing but text entry. `~` and `?` are printables a subject
   or a body has to be able to hold, so the form takes `F1` alone, and `~` joins
   `?` and `F1` on the global layer where nothing is being typed.
2. **Help had to be told the truth first.** It read one context, so from the
   composer it would have listed `q :quit` — dead inside a form — and omitted
   `Tab`, `Esc` and the editor keys, which are the ones that fire. A hint that
   sends the user to a lying overlay is worse than a cropped hint.
3. **The other surfaces keep their inline hints.** The explorer, pager, log and
   attach preview each say four things or fewer and fit. The complaint was
   quantity, not the mechanism.

## 4. Plan

### 4.0 The settled design

§2 predates the discussion. What follows is what §3 settled, and it is what the
phases below build.

**The surface.** One `FormSpec` per compose session, hosted in `ActiveForm` for
as long as the session lives, drawn by `compose_layout` — the reading column
beside a reply, over the panes otherwise. Tab order:

| stop        | kind                                      |
| ----------- | ----------------------------------------- |
| From        | text, read-only, dimmed, no cursor        |
| To          | address text + completion                 |
| Cc          | address text + completion                 |
| Bcc         | address text + completion                 |
| Subject     | text                                      |
| Attachments | horizontal list; Left/Right within        |
| Body        | textarea, claims the remaining height     |
| Discard     | button (`ButtonRole::Cancel`, relabelled) |
| Send        | button (`ButtonRole::Primary`)            |

All seven fields always show, at every width. The attachment row with nothing
attached is an `Add Attachment` affordance: Enter on it opens the explorer.
Enter on an attachment previews it, Delete detaches it.

**Keyboard.** The compose form owns the keyboard, as the composer does today.
Key resolution becomes a layered stack rather than "context, then global":
`[editor,] form, compose, global` — the `editor` layer present only while the
body field has focus, so its arrows and Ctrl bindings beat the form's without
either being rewritten. Consequences:

- `<Enter>` on a field moves focus forward (R2 q2, option 1); on a button it
  activates. In the body it is `:editor-newline`, bound in the `editor` layer,
  so it never reaches `:form-activate`. Send is only ever the button or
  `Alt-Enter`.
- `<Esc>` is `:form-cancel` everywhere, including the body, and the composer's
  cancel is the discard confirm. `:editor-done` is deleted — there is no mode
  to leave.
- The seven `Alt` composer commands live in the `compose` context, which layers
  under everything, so they answer from any field.

`cancel()` closes the form before running `on_cancel`, which would destroy a
composition the user is only being _asked_ about discarding. `CancelFn` gains a
`Close | Keep` outcome: compose returns `Keep` and opens the confirm, whose
accept branch closes the form.

**Attachments invert.** `session.attachments` becomes the store, mutated by
attach and detach; `sync_attachments` (which derives it from body tokens) is
deleted. A token in the body is a **position marker only** in v1: `build` goes
on stripping token lines and attaching each listed path as an ordinary
attachment, so an attached-but-unplaced file still reaches the wire and deleting
a token no longer detaches anything. Real inline parts (`Content-Disposition:
inline`, `cid:`, `multipart/related`) are deferred — this is my reading of the
bare "confirm" on R2 q6's three-part question; say so if you meant the MIME work
in v1.

**Validation.** R2 q9 supersedes the R1 answer: **no field validators.**
Validation stays where it is — `build` reports unparseable headers, `postpone`
goes on tolerating an unaddressed draft, and `send_with_checks` keeps the
empty-subject and forgotten-attachment confirms.

**Completion** moves from the bottom panel to directly below the focused field,
clamped inside the host rect when there is no room, for every completed form
field. The command line keeps its own bottom panel.

**Write-through.** Headers reach `ComposeSession` on every keystroke. The body
reaches `session.body` on every keystroke but the body _file_ and its sidecar
are throttled: at most once a second, plus on focus-out, send, postpone and
discard.

**Deleted by this feature.** `ComposeStage`, `InputMode::Editor`,
`open_headers_form`, `prompt_header`, `sync_attachments`,
`ComposeOp::{To, Cc, Bcc, Subject, EditBody}`, the `:compose-to`/`-cc`/`-bcc`/
`-subject`/`-edit` commands, and `:editor-done`. `$EDITOR` survives as
`:compose-edit-external` on `Alt-e`.

### 4.1 Phase 1 — layered key resolution

`Keymaps::resolve_layered` takes a slice of contexts instead of one plus an
implicit `global`. Every existing caller passes `[context, global]`, so nothing
changes behaviourally. Tests: a two-layer stack resolves the more specific
binding, falls through when unbound, and reports `Prefix` from either layer.

### 4.2 Phase 2 — form placement

`FormPlacement::{Overlay, Host { layout, order }}` on `FormSpec`, defaulting to
`Overlay`. `form_geometry` takes the placement: `Overlay` keeps
`centered_panel`, `Host` uses the rect it is handed. Entity `WidgetOrder` comes
from the placement rather than the `layer::OVERLAY` constants. Existing forms
are untouched. Tests: a hosted form's controls all land inside the host rect;
an overlay form's geometry is unchanged.

### 4.3 Phase 3 — variable-height fields

`FieldSpec` gains a row count; `FormMetrics` carries per-field heights instead
of `field_count`; one designated field may claim the remaining height. Every
existing field is one row, so the existing geometry tests must pass unchanged.
New tests: a tall field pushes the rows below it, and the flexible field shrinks
rather than overflowing a short frame.

### 4.4 Phase 4 — the body field

`FieldKind::Body`, whose `FieldRuntime` holds the `SharedArea` that
`compose/inline.rs` already defines, rendered through the existing
`style::paint_lines`. `EditorOp` dispatch moves from `InlineEditor` to the
focused body field; the `editor` layer is pushed when the focused field is a
body. `<Enter>` binds to a new `:editor-newline`; `:editor-done` goes. Tab is
refused by the runtime so it always reaches `move_focus`.
`tests/inline_editor.rs` is re-pointed at the field rather than the mode — its
21 behaviours all survive.

### 4.5 Phase 5 — compose as the form

`compose/form.rs` builds the spec above and opens it from `start_compose`,
`start_compose_to`, `start_reply`, `recall` and `recover`. Write-through on
change; the `Keep`-outcome cancel guard; the `compose` context reduced to the
`Alt` commands. Delete what §4.0 lists. `render.rs` keeps only the cheat-sheet
footer, which the spec supplies to the form frame. `tests/compose.rs`,
`tests/reply.rs` and `tests/drafts.rs` drive Tab and typing instead of the
prompt chain.

### 4.6 Phase 6 — the attachment list

`FieldKind::Attachments` reading `session.attachments`: one tab stop,
Left/Right between items, Enter to preview (empty row: open the explorer),
Delete to detach. `Alt-a` opens the explorer, `Alt-i` inserts a token for the
selected attachment at the cursor, `Alt-d` detaches. `sync_attachments` is
deleted and `attach_prompt` with it. Tests: attaching without inserting still
builds an attachment; deleting a token leaves the file attached; detaching
removes both the file and any token naming it.

### 4.7 Phase 7 — completion below the field, and throttled persistence

`panel.rs` anchors to the focused field's rect from `form_geometry`, clamped
inside the host rect. `persist_session` gains the throttle and the flush points.
Tests: a completion panel for the bottom-most field stays inside the frame; a
crash after the throttle window still recovers the body.

### 4.8 Phase 8 — help instead of a border hint

Added after R3, once the branch was already verified.

1. `Keymaps::help_rows` takes the layer stack rather than one context, dropping
   any sequence an earlier layer already answers — the same rule
   `resolve_layered` fires by.
2. `Surface::key_layers` names each surface's layers in one place, and every
   surface's key handler resolves through it, so help and the router cannot
   disagree. `surface::key_layers` is the live stack: the top surface's, or the
   focused pane's when nothing is above it.
3. `~` and `<F1>` join `?` on the global layer; `<F1>` alone joins the form
   layer.
4. `FormSpec::with_hint` and the `FormState` / `FrameView` plumbing behind it
   are deleted — the composer was its only caller.

### 4.9 Documentation

`keys.toml` defaults, the help overlay, and `documentation/specification.md`'s
Compose section (which still says "External `$EDITOR` composing" first) are
updated in the phase that changes each behaviour, not at the end.

## 5. Verification

Behaviour changed here by design, so "unchanged" was never the bar. What was
proven instead, at every phase:

- `cargo clippy --workspace --all-targets` clean throughout. `--all-targets`
  matters: a plain `cargo clippy` skips `#[cfg(test)]`, and phase 3 shipped a
  signature change whose only breakage was in tests.
- `CARGO_INCREMENTAL=0 cargo test --workspace` green at every commit. **630
  passing** at the end (352 unit + 278 across 24 integration suites), up from
  600 on main.
- Each phase before the switch left the old composer working: phases 1–4 added
  layered keys, hosted placement, filling fields and the body field without any
  of it being reachable, and every existing test went on passing untouched.
- The behaviours the old suites pinned were re-pinned rather than dropped. All
  21 inline-editor tests survive as `tests/body_field.rs`, re-pointed at the
  field: motions, undo, selection, the clipboard, the token preview and the
  swallowed chord all still assert the same outcomes.

Phase 8 pinned its own claims: help over a compose-form stack keeps the
editor's `Enter` and drops every global, `F1` opens help from inside the form,
and `~` still types a `~` into a field.

Per-suite at the end: compose 14, body_field 19, reply 7, drafts 4, outbox 4,
contacts 15, index 22, pager 18, overlay 14, delete 6, help 6, sidebar 5,
keymap_layout 4, plus the rest.

## 6. Implementation Report

Seven phases, seven commits, plus documentation and cleanup — and then an
eighth phase from R3, after the branch was already verified. The plan held —
nothing was reordered except attach, and no phase had to be unpicked.

### What was built

`resolve_layered` takes an ordered slice of contexts, so the composer's key
resolution is a stack — `editor` (when a body has focus), `form`, `compose` —
rather than a special case. `FormPlacement::Host` lets a form take a rect
instead of centering itself, and the chrome pins to the bottom so a tall host
puts its slack above the buttons. `FieldHeight::Fill` gives the body the rows
the headers leave. `FieldKind::Body` is a real `ratatui-textarea` inside a form,
and `FieldKind::Entries` is the attachment row.

Phase 8 finished the thought the stack started: `Surface::key_layers` is now the
one place a surface names its layers, its own handler and the help overlay both
read it, and `help_rows` merges a stack the way `resolve_layered` resolves one.
Help therefore lists exactly what will fire — over the composer that is the
editor's keys, the form's and the composer's, and none of the globals.

### Decisions taken during the work

- **Enter-steps-forward is opt-in.** Applying R2 q2 globally broke 15 tests
  across the account wizard, where "type, Enter, next" is the flow. The answer
  was about the composer, so `FormSpec::stepping_enter()` is per-form and the
  composer takes it. Wizards keep Enter as their primary action.
- **Attach moved to the explorer in phase 5, not 6.** `:attach` opened a form,
  which would have clobbered the compose form the moment it was pressed — the
  collision R2 q1 settled. Shipping that broken across a phase was not worth the
  tidier boundary.
- **`ui.compose.editor` is gone.** It chose inline versus `$EDITOR` for a body
  that is now always a field, so it selected nothing. `$EDITOR` survives as
  `Alt-e` / `:compose-edit-external`. This is a user-visible config removal that
  was not in the plan.
- **The frame carried a hint, and then did not.** The cheat sheet died with the
  review screen, and forms do not fall through to global bindings, so `?` types
  a `?` while composing. `FormSpec::with_hint` put the composer's Alt commands
  along the bottom border — seven of them, which no frame is wide enough to
  hold. R3 replaced it with `F1`, and the hint plumbing is gone (§3.6, §4.8).
  The lesson worth keeping: one border row is not a place to put a list that
  grows.
- **`CancelFn` returns `Close | Keep`.** `cancel` closed the form before running
  the callback, which would have destroyed a composition the user was only being
  *asked* about discarding. Compose returns `Keep` and the confirm closes from
  inside its own answer.
- **An empty body is no lines, not one empty line.** The field always holds a
  line for the caret; a session with nothing in it holds none. Until they were
  normalized, every frame looked like a change and burned the throttle's leading
  write.

### Follow-ups

- Real inline parts — `Content-Disposition: inline`, `cid:`,
  `multipart/related` — are still deferred; a token is a position marker and the
  MIME stays a plain attachment (the R2 q6 reading recorded in §4.0).
- Switching From mid-composition is still v2: the field is read-only.
- `:compose-edit` as a "zoom the body to full height" toggle was dropped rather
  than reimplemented. Nothing asks for it yet.
- The placement captures sidebar visibility and `ui.pager.max_width` when the
  form opens. Neither can change while composing — the composer owns the
  keyboard — but a mouse-driven sidebar toggle would not re-place the form.

## 7. Testing and Cleanup

The cleanup skill ran over `compose/` and `overlay/form/`. It found two things
the compiler could not:

- **`compose/render.rs` was orphaned.** Phase 5 removed its systems and its
  `mod` declaration but left the file: 478 lines of the old review screen sat on
  disk, uncompiled, invisible to both clippy and `dead_code`. Deleted.
- **Enter on the attachment row previewed the wrong thing.** It called
  `preview::open`, which reads the token under the *body* caret, rather than the
  entry the row had picked. Now `preview::open_path` takes the selection.

Comments reworded, all of them describing something that no longer exists: the
`persist` module doc and its recover notice ("back to review"), `reply`'s
"straight to the editor", `ComposeSession::write_body`'s "outside the editor",
`build`'s "tokens declare the MIME parts", and the `overlay::form` module doc,
which called every form modal.

Verified after cleanup: `cargo fmt --all`, clippy clean on `--all-targets`, 627
tests passing. Re-verified after phase 8: same three, 630 passing.

Phase 8's own cleanup removed `FormSpec::with_hint`, `FormState::hint`,
`ActiveForm::hint` and `FrameView::hint` rather than leaving a facility no form
uses. `FrameChrome::hint` stays: it is the ui-kit's, and the explorer, pager,
log and attach preview still say their four keys on the border.
