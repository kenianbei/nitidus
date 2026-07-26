# feature - Overlay Forms - v1

Account creation becomes a modal overlay form — a stepped, multi-page surface
of tab-focusable fields with Back / Next / Cancel buttons — and serves as the
proving ground for a systematic overlay layer: focus order, mouse hover and
press, buttons, and named elevation. This is the follow-up feature-overlay-v1
§3.3 R2 promised ("per-field keyboard focus remains available to future
multi-field surfaces via the same components"), and it builds the machinery the
parked refactor-ui-v1 expects to consume for confirmations. The overarching
goal: the bottom bar stops hosting data-entry prompts and keeps only what
belongs to it — the statusline, `:` commands, and incremental `/` search.

## 1. Current Design

### The account wizard is thirteen bottom-bar round-trips

`accounts/wizard/mod.rs` (613 lines) chains `text_step` prompts and pickers
through closures carrying a `Draft`: name → email → provider picker → (Gmail /
Outlook / Custom IMAP branches) → imap host → smtp host → folder names → auth
picker → OAuth provider picker → client id → client secret → password command →
display name. Validation failures re-open the same prompt; `:edit-account`
replays the chain prefilled; a zero-account start enters it automatically. Each
step is one line of context — the user never sees the whole shape of what they
are filling in, and there is no way back: `Esc` cancels the entire run.

### The bottom-bar prompt

`prompt/mod.rs` (439 lines) + `prompt/panel.rs`: an `InputMode::Prompt` router
gate, editing through tui-prompts `TextState`, masking (`PromptRequest::
masked()`, used by the password prompt in `accounts/mod.rs`), Tab-cycled
completions with a panel at order 90, `on_submit`/`on_cancel` closures. Twelve
files call `open_prompt`: the wizard, account manage/passwords, compose
(headers, attach path, discard and send confirms, forward-To), contacts
(add/edit/mutate/transfer), and index delete confirms.

### Modal surfaces exist, each hand-rolled

- **Picker** (`overlay/mod.rs`): resource-driven entity spawn/despawn, order
  100, rebindable single-key `picker` context, unbound printables type into the
  filter, mouse hover + click through plurimus, `UiFocusable` +
  `UiFocusMessage::Set`/`Clear` on open/close.
- **Explorer** (`explorer/mod.rs`): own resource and router gate, hardcoded
  keys, **no explicit `WidgetOrder`**.
- **Attach preview** (`compose/preview.rs`): order 110, any-key dismiss.

The router now checks four input modes plus three modal resources, in a fixed
order that is itself an undocumented stacking rule.

### Elevation is an implicit ladder of magic numbers

Completion panels 90 (`cmdline/panel.rs`, `prompt/panel.rs`), picker 100,
preview 110, toast 120, explorer unset, base widgets 0. The ladder works but is
defined nowhere; two files independently declare `PANEL_ORDER = 90` and two
declare different `OVERLAY_ORDER`s.

### plurimus already ships the interaction vocabulary

`UiFocusable { tab_index, enabled }`, `UiFocusMessage::{Next, Prev, First,
Clear, Set}`, `UiHoverable`/`UiPressable`/`UiDisabled`, and the derived
`UiFocused`/`UiHovered`/`UiPressed` markers, with pointer hit-testing over
`WidgetRect` + `WidgetOrder`. The app uses draw order, mouse hit-testing, and a
single `UiFocusable` on the picker; tab order, `UiPressable`, and `UiDisabled`
are unused.

`WidgetLayout` `#[require(WidgetRect)]`, and every layout fn receives the whole
terminal rect — so any entity that wants its own hit-testable box needs its own
`Widget` + `WidgetLayout` pair. Per-field entities are therefore not a stylistic
choice; they are what plurimus hover, press, and click-to-focus require.

### vcard_tui shows the target shape

A dialog is a *set of entities* keyed by a marker enum: a popup frame, one
entity per field, Cancel/Submit buttons — each with a builder
(`popup_builder`, `text_prompt_builder` over tui-prompts `TextState`,
`button_builder`), a focus index, and a `UiInteractiveSync` closure that maps
the `UiFocused`/`UiHovered`/`UiPressed`/`UiDisabled` markers onto the widget's
visual variant each frame. Spawn/despawn follows a state resource. The delta,
recorded in feature-overlay-v1 §3.3 R2: vcard_tui delivers keyboard input
per-entity through plurimus; nitidus keyboard stays on the router (rebindable,
no double delivery) with plurimus handling the mouse path only.

### What the theme can express

`ThemeColorStates` has `normal`/`disabled`/`focused`/`hovered`/`selected`, all
produced by `ThemeColorStates::derive` from a single seed and already asserted
mutually distinct. Only `pressed` is missing. (An earlier draft of this document
claimed `hovered` was absent too; it is not — the picker's row hover already
uses it.)

## 2. Proposal

### 2.1 A stepped form overlay subsystem

`overlay/form/` beside the picker. A form is one modal surface hosting an
ordered set of **pages**; each page holds fields and the frame carries the
buttons.

```rust
FormSpec {
    title: String,
    mode: FormMode,          // Create | Edit
    primary_label: String,   // "Create" | "Save"
    pages: PagesFn,          // Fn(&FormValues) -> Vec<PageSpec>
    on_submit: SubmitFn,     // Fn(&mut World, FormValues)
    on_cancel: CancelFn,
}

PageSpec  { id: &'static str, title: String, fields: Vec<FieldSpec> }
FieldSpec { id: &'static str, label: String, kind: FieldKind,
            span: FieldSpan, initial: String, validate: Option<ValidateFn> }
FieldKind { Text { masked: bool }, Select { options: Vec<SelectOption> } }
FieldSpan { Full, Half }     // two Half fields share a row
```

Values live in one id-keyed `FormValues` map, not in the entities, so a field
keeps what you typed across page derivation, page switches, and respawns. Text
fields edit through tui-prompts `TextState`, the same engine the bottom prompt
uses, so masking comes free.

An `ActiveForm` resource drives a vcard_tui-style spawn/despawn system that
builds the entity set for the *current page*: frame (Clear + themed bordered
block, carrying the step strip and the message row), one entity per visible
field, one per button. Every interactive entity carries `UiFocusable` with its
tab index; buttons also carry `UiHoverable` + `UiPressable`.

`form/geometry.rs` owns the layout math as pure functions, and both the entity
layout fns and the renderer call it — the picker's `rows_geometry` discipline
generalized, so click math can never drift from the drawing.

### 2.2 Steps derived from the draft

`PagesFn` recomputes the page list from the current values, so branching is
data rather than control flow: choosing Gmail drops the Servers page, choosing
Password (keyring) drops the OAuth client page. The provider and auth choices
therefore have to live *inside* the form, which is what `FieldKind::Select` is
for; the two wizard pickers are absorbed rather than interrupting the form.

Recomputation runs on every value change, but the entity set is rebuilt only
when the derived shape changes — the `Vec<PageId>` or the current page's
`Vec<FieldId>`. Typing never churns entities or focus.

Navigation follows the mode:

- **Create** — gated. `Next` validates the current page before advancing;
  steps not yet reached carry `UiDisabled`, so neither Tab nor a click can
  jump ahead. `Back` is always available.
- **Edit** — free. Every step is immediately reachable by strip click or
  `PageUp`/`PageDown`; `Save` validates all pages and focuses the first
  failing field, switching pages if needed.

Validation messages render in a fixed message row above the buttons, and the
offending field's label takes the error style. A reserved row keeps the
geometry stable rather than reflowing the form under the cursor.

The step strip is hand-rolled as a single row of styled spans with a
`step_at(x)` helper mirroring the renderer. `ratatui-comfy-tabs` was the
obvious candidate — it is already a dependency, does compact 1-row strips, and
`tab_index_at` would give click-to-jump free — but its `style`/`highlight_style`
are per-widget, and gated creation needs *per-tab* disabled styling. Thirty
lines of spans buys that; the dependency does not.

### 2.3 Keyboard: a rebindable `form` context, focus owned by the form

The router gains a form gate resolving single keys against a new `form` keymap
context (the picker precedent — no chord waits, because unbound printables must
type, and no global fallback, so global bindings never leak through a modal).
Defaults:

| Key                | Command             |
| ------------------ | ------------------- |
| `Tab` / `S-Tab`    | `:form-focus-next` / `:form-focus-prev` |
| `Down` / `Up`      | `:form-focus-next` / `:form-focus-prev` |
| `Left` / `Right`   | `:form-left` / `:form-right` |
| `Enter`            | `:form-activate`    |
| `Esc`              | `:form-cancel`      |
| `PageDown` / `PageUp` | `:form-next-page` / `:form-prev-page` |

`:form-left`/`:form-right` dispatch on the focused field's kind — cursor motion
in a Text field, option cycling in a Select — so both stay rebindable without
the router knowing about field kinds. Unbound printables and Backspace edit the
focused Text field's `TextState`.

`:form-activate` fires the page's primary action: `Next` on a non-final page
during creation, otherwise the spec's primary label; on a focused button it
activates *that* button. One reflex — Enter always does what the highlighted
button says.

**The form owns focus, not plurimus.** `PlurimusUiPlugin` chains its PreUpdate
systems `… collect_key_actions → focus_on_pointer → run_world_intents →
apply_focus_intents …`, so the router (which executes inside
`run_world_intents`, draining *every* key collected that frame) runs before
`UiFocusMessage` is applied. A `Tab` and the keystrokes after it arriving in
one 16 Hz frame would all route against the pre-Tab focus — precisely the
burst-input bug `router.rs` was built to avoid. So `ActiveForm.focused:
FieldId` is authoritative and moves synchronously; a sync system mirrors it
outward with `UiFocusMessage::Set` for styling and hit-testing. `UiFocusable`
and `UiDisabled` still go on every interactive entity (they drive plurimus's
disabled sanitizing), but `UiFocusMessage::Next`/`Prev` are not on the keyboard
path. This retires §2.2 of the first draft, which expected Tab to be "the first
real use of plurimus tab order".

### 2.4 Mouse: hover, press, click-to-focus

Mouse stays plurimus-delivered per app convention. Field and button entities
take a `targeted` mouse binding, which also runs inside `run_world_intents` and
therefore sets `ActiveForm.focused` on the same synchronous path as the
keyboard. Buttons take hover and press styling from the marker components and
fire on release. A `UiInteractiveSync`-style system (the vcard_tui pattern,
generalized) maps `UiFocused`/`UiHovered`/`UiPressed`/`UiDisabled` onto each
widget's visual state every frame, so every overlay surface gets consistent
interactive styling from one system rather than per-widget wiring.

`ThemeColorStates` gains a `pressed` variant, derived from the seed alongside
the others. Presets all build through `derive`, so this touches `states.rs` and
its derive test only.

### 2.5 Named elevation

A `layer` module in `nitidus-ui-kit` defining the ladder once: `BASE = 0`,
`PANEL = 90`, `OVERLAY = 100`, `MODAL = 110`, `TOAST = 120`. Existing consumers
migrate; the explorer gains an explicit `OVERLAY`. Forms sit at `OVERLAY`.

The stacking rule the router encodes becomes stated rather than accidental:
modal gates are checked outermost-first, and a surface that can open above
another must draw above it. Absorbing the wizard's pickers into Select fields
removes the one live instance of picker-over-form, but the rule stays — it is
free, and confirmations will reintroduce the case.

### 2.6 The account wizard as proving ground

Thirteen prompt steps become one form of at most four derived pages:

| Page             | Fields                                                    |
| ---------------- | --------------------------------------------------------- |
| **Account**      | name, email, display name                                 |
| **Provider**     | provider (Select), auth method (Select)                   |
| **Servers**      | imap host, smtp host, drafts/sent/trash/archive folders   |
| **Credentials**  | OAuth provider (Select) + client id + secret, *or* password cmd |

Servers appears only for Custom IMAP; Credentials' shape follows the auth
choice and disappears entirely for keyring auth. `:edit-account` opens the same
form in `Edit` mode, prefilled, with every page immediately reachable — the
change this feature is really for. The zero-account start opens it over the
empty index. The password prompt (`accounts::set_password`) becomes a
one-field masked form and is the subsystem's first consumer, well before the
wizard lands.

Most of `wizard/mod.rs` disappears: the closure chain is replaced by a page
derivation function over `Draft`, so the net line count across the branch is
expected to be flat or down despite the new subsystem.

### 2.7 What the bottom bar keeps

This doc migrates only the wizard and the password prompt. The end state it
points at: the bottom row is the statusline, the `:` command line, and
incremental `/` search (a mode, not a prompt — vim/less precedent, deliberately
staying). Recorded as follow-ups, not scope: compose header prompts, contact
add/edit prompts, and the y/n confirms — the confirms belong to refactor-ui-v1's
"confirmations as overlays" item, which consumes this machinery.

Out of scope: porting any other prompt caller, confirmation overlays, toast
routing (refactor-ui-v1 item 3), completions inside form fields (the
address-book Tab-cycle stays a bottom-prompt feature until a form needs it),
and leader menus.

`documentation/specification.md` needs no change: "Modal per-context keymaps"
already covers a new `form` context, and the account wizard is already a listed
feature — this changes its presentation, not the product surface.

## 3. Discussion

### 3.1 R1 Questions

1. **Doc structure.** This feature ships the form/interaction machinery with
   account creation as its proving ground, and refactor-ui-v1 stays parked
   until it can consume the result for confirmations. Or would you rather
   unpark refactor-ui-v1 first and make this a child of it?
2. **Step consolidation.** §2.5 collapses thirteen steps into three forms plus
   the existing pickers, which changes the wizard's shape as well as its skin.
   Comfortable, or should v1 port more faithfully (one form per current
   text-run) and consolidate in a later pass?
3. **Enter semantics.** Proposal: `Enter` on a field submits the form (primary
   action), `Tab` is how you advance, `Enter` on a focused button activates
   that button. The alternative — Enter advances to the next field, submit
   only from a button — is more form-like but slower for the common case.
   Which?
4. **Theme.** Interactive styling needs hover and press to be visually
   distinct. Add `hovered`/`pressed` variants to `ThemeColorStates` (touches
   every theme consumer's struct, presets fill them from existing colors), or
   map them onto the existing `focused`/`selected` variants and keep the theme
   untouched?
5. **Layer module.** Names and home as proposed in §2.4 (`ui-kit::layer`,
   BASE/PANEL/OVERLAY/MODAL/TOAST)? And should existing consumers migrate in
   this doc, or is that a separate chore?
6. **Field engine.** Reuse tui-prompts `TextState` per field (the bottom
   prompt's engine, masking included) — confirm, or would you rather the
   fields share the richer ratatui-textarea now that it is a dependency?
7. **Button activation keys.** Besides Enter-on-focused-button: should the
   buttons also have direct keys while the form is open (e.g. `C-s` submit),
   or is Tab-to-button + Enter (+ mouse) enough for v1?

### 3.2 R1 Answers

1. **Keep as proposed.** This doc owns the machinery; refactor-ui-v1 stays
   parked and consumes it later for confirmations.
2. **Superseded by R2** — see below.
3. **Answered in R2** once steps changed what "submit" means.
4. Question was built on a false premise: `hovered` already exists. Resolved in
   R2 — add `pressed` only.
5. **As proposed.** `ui-kit::layer`, those five names, existing consumers
   migrated inside this doc rather than deferred to a chore.
6. **tui-prompts `TextState`.** Same engine as the bottom prompt; masking comes
   free. ratatui-textarea stays the inline *body* editor's engine — a form field
   is single-line and does not want it.
7. **No extra chords in v1.** Tab-to-button + Enter + mouse. The `form` context
   is rebindable, so a user who wants `C-s` can bind `:form-activate` to it
   without the defaults claiming the key.

### 3.3 R2 Questions

Raised by the user against the R1 draft: *"Would it be easier to add a tabbed
step wizard as well, so we can have multi-step forms on the overlay?"*

Assessment: not easier as a subsystem — a page container, step strip, Back/Next
and per-page validation are all additions — but better, and it makes the wizard
itself much smaller. What inflates `wizard/mod.rs` to 613 lines is the closure
chain, and the R1 plan of three chained forms preserved that chain at a third of
the length. A stepped container deletes it: the form owns one `Draft` and steps
become a `Vec<PageSpec>` rather than control flow. Two arguments decided it:
`:edit-account` is not a wizard and should not force-march anyone through pages
to change one IMAP host; and a chain has no `Back`, while steps give it free.
Branching is answerable by deriving the page list from the draft, which requires
Select fields and absorbs the two pickers.

1. **Form shape.** Stepped container with Select fields absorbing the pickers;
   stepped with pickers left as separate interrupting overlays; or the original
   three chained forms?
2. **Enter, with steps in play.** The page's primary action (Next / Save);
   always save the whole form; or advance the field?
3. **Step navigation.** Gated during creation and free during editing; always
   free; or always gated?
4. **Press styling.** Add `pressed` to `ThemeColorStates`; reuse `selected`; or
   no press styling in v1?

### 3.4 R2 Answers

1. **Stepped container with Select fields.** One overlay for the whole wizard,
   pages derived from the draft, pickers absorbed.
2. **Enter fires the page's primary action** — `Next` on a non-final creation
   page, otherwise `Create`/`Save`; on a focused button, that button.
3. **Gated on create, free on edit.** Unreached steps are `UiDisabled` during
   creation; `Back` always works; editing reaches every page immediately and
   validates everything on `Save`.
4. **Add `pressed` to `ThemeColorStates`**, derived like its siblings, and
   correct §1's claim that `hovered` is missing.

## 4. Plan

Each phase leaves the workspace compiling and the suite green.

### Phase 1 — Foundations, no behavior change

- `nitidus-ui-kit/src/layer.rs`: `BASE`/`PANEL`/`OVERLAY`/`MODAL`/`TOAST`.
- Migrate `toast.rs`, `cmdline/panel.rs`, `prompt/panel.rs`,
  `compose/preview.rs` and the picker off their local constants; give
  `explorer/mod.rs` an explicit `OVERLAY`.
- Add `pressed` to `ThemeColorStates` + `derive`, extending the
  distinctness and monotonicity tests.
- Split `overlay/mod.rs` into a small `overlay/mod.rs` (plugin, stacking-rule
  doc comment, re-exports) plus `overlay/picker/{mod,render,mouse}.rs`. The
  re-exports keep `crate::overlay::open_picker` &c. resolving, so none of the
  ten calling files change.

### Phase 2 — Single-page form subsystem

- `overlay/form/`: `spec.rs`, `state.rs`, `geometry.rs`, `entity.rs`,
  `render.rs`, `mouse.rs`, `interaction.rs` — Text fields only, no steps,
  Cancel + one primary button.
- `CONTEXT_FORM` + `DEFAULT_FORM_BINDINGS`; `Action::Form(FormOp)` and its
  commands; the router's form gate after the picker gate.
- First consumer: `accounts::set_password` becomes a one-field masked form.
- Tests: tab order and wrap; Enter submits; Esc cancels; masking renders `*`;
  a failing validator focuses its field and shows the message; a click focuses
  the field under the pointer; a button fires on release, not press.

### Phase 3 — Select fields

- `FieldKind::Select`, rendered `‹ Gmail ›` with the option detail dimmed
  beside it; `:form-left`/`:form-right` cycle it and move the cursor in Text
  fields.
- Tests: cycling wraps both ways; the selected option lands in `FormValues`.

### Phase 4 — Pages and the step strip

- `PagesFn` derivation over id-keyed `FormValues`; respawn only on shape
  change.
- Step strip render + `step_at`; `Back`/`Next`; `UiDisabled` on unreached
  steps during creation; free navigation in `Edit`.
- Per-page validation on `Next`, whole-form on `Save` with a jump to the first
  failure.
- Tests: values survive a page switch and a shape change; a Select flip adds
  and removes a page; gated creation refuses to jump ahead while edit mode
  allows it; `Save` with an error on page 1 focuses that field from page 3.

### Phase 5 — Wizard migration

- Rewrite `accounts/wizard/mod.rs` as one `FormSpec` over `Draft`; delete the
  `text_step` chain and the two picker call sites.
- `:edit-account` opens the same form in `Edit` mode; the zero-account start
  opens it over the empty index.
- Port the four existing wizard tests to drive the form, keeping their
  assertions about written config, presets, folder overrides, duplicate names,
  and the password/OAuth chaining.

### Phase 6 — Verification and cleanup

- `cargo clippy --workspace` clean, `CARGO_INCREMENTAL=0 cargo test
  --workspace` green with pass counts recorded in §5.
- Run the cleanup skill over the new module; fill in §§5–7.

## 5. Verification

Measured at the branch point (`5a987a2`, the design doc commit) and again after
Phase 6:

| Command                                  | Before     | After      |
| ---------------------------------------- | ---------- | ---------- |
| `cargo test --workspace` (passed/failed) | 472 / 0    | 545 / 0    |
| `cargo clippy --workspace --all-targets` | clean      | clean      |
| `cargo fmt --all --check`                | clean      | clean      |

Test runs used `CARGO_INCREMENTAL=0`, per `rules/testing.md`.

Seventy-three net new tests. Each phase was verified green before the next
began, so no phase left the workspace broken.

This is a feature, not a refactor, so behavior is deliberately not preserved.
The intended changes are in §§2.6–2.7; the unintended ones found along the way
are in §6.

## 6. Implementation Report

### What landed

All six phases, in order, each its own commit:

1. `nitidus-ui-kit::layer` with the five named rungs; toast, both completion
   panels, the attach preview and the picker migrated onto it; the explorer
   given the `WidgetOrder` it never had; `pressed` added to
   `ThemeColorStates`; the picker moved into `overlay/picker/` behind
   re-exports, so none of its ten callers changed.
2. `overlay/form/` — single-page forms with focus, validation, buttons and
   mouse. `:set-password` became the first consumer.
3. `FieldKind::Select`.
4. Derived pages and the step strip.
5. The wizard rebuilt on top of all of it.
6. Cleanup and this report.

### Three findings that changed the design

**plurimus applies focus a frame late.** `PlurimusUiPlugin` chains PreUpdate as
`collect_key_actions → run_world_intents → apply_focus_intents`. The router
executes inside `run_world_intents` and drains *every* key collected that
frame, so a `UiFocusMessage` written by a Tab is not applied until after the
keys following it have already been routed. At 16 Hz that is a ~62 ms window in
which a fast typist or a paste would put text in the field they just left.
`ActiveForm` therefore owns focus synchronously and mirrors it outward, and
§2.2 of the first draft — which expected Tab to be "the first real use of
plurimus tab order" — was retired before any code was written. `UiFocusable`
still earns its place: it drives plurimus's own hit-testing and disabled
sanitizing, which is how an unreached step gets refused the pointer for free.

**Page derivation needs a fixpoint, not a pass.** Seeding a page's defaults can
bring another page into existence, and that page has defaults of its own — a
prefilled `:edit-account` for a custom IMAP account seeds `provider = custom`,
which is what makes the Servers page exist at all, which is what has the folder
defaults. A single derive-then-seed left those fields empty. `converge_pages`
now loops derive → seed until the values stop changing (capped at eight rounds
against a pathological `PagesFn`).

**`<S-Tab>` was unreachable in every context.** A terminal reports
Shift-Tab as `BackTab`, never as Tab with a shift modifier, but crokey parses
`<S-Tab>` literally into Tab+SHIFT — a combination no key event can produce. The
form's back-focus binding was therefore dead on arrival, and so was any
`<S-Tab>` a user might write in `keys.toml` for any context.
`parse_key_sequence` now folds Tab+SHIFT onto `BackTab`, so `<S-Tab>` and
`<BackTab>` compile to one binding; `<C-S-Tab>` is deliberately excluded,
being a distinct key terminals really do send as Tab with modifiers. The
original test for this passed either
way — it asserted through Enter semantics that held whether or not focus moved —
which is why it never caught the defect; it now discriminates by typing after
the keypress and checking the character went nowhere. The help overlay lists
the binding as `BackTab`, which parses back.

**A shape change must not move focus.** Rebuilding the control set originally
reset focus to the first field. Cycling a select that adds a page therefore
yanked focus off the select mid-keypress, so the *next* Right arrow silently
edited a different field. Found by a wizard test that walked provider → auth
and got the wrong answer. `rebuild_fields` now keeps focus on the same field
*id* where it survives, and `go_to_page` deliberately clears it first so a page
switch still lands on the new page's first field.

### Where the estimate was wrong

§2.6 predicted the wizard's line count would come out "flat or down" because
the closure chain would disappear. It disappeared, and the count still went up:
509 production lines before (`wizard/mod.rs` less its tests, plus `presets.rs`)
against 597 after (`mod.rs` 232 + `fields.rs` 314 + `presets.rs` 51). The
declarative page/prefill mapping costs more than the chain did, mostly because
`Prefill` round-trips an `AccountConfig` into strings and the form's answers
back into one. The prediction was wrong; the trade was still worth making,
since what the extra lines buy is Back, free navigation when editing, and
per-step validation. `overlay/form/` itself is 11 files and ~3,150 lines, a
little under half of that tests.

### Behavior changes worth knowing

- Saving an account whose SMTP host was never set is now refused until one is
  supplied. The old chain always wrote one, so this only bites accounts hand-
  written into `config.toml` without an `[outgoing]` block — surfaced by a test
  fixture that had exactly that shape.
- A new account defaults to Gmail over OAuth2, so the Credentials step is
  present from the moment the form opens. Choosing keyring auth drops it again.
- `:set-password` rejects an empty secret with an inline message and keeps the
  form open, rather than closing and warning on the statusline.
- Forms are gated on their own resource rather than an `InputMode`, which makes
  them immune to the command-line ordering bug that `:set-password` used to
  need a regression test for. That test now pins the immunity instead.

### Follow-ups, not done here

- Field value scrolling windows by character count, so a long value containing
  wide (CJK) characters can scroll a column or two off. The picker has the same
  limit.
- `LABEL_WIDTH` is a fixed 18 columns; a form whose labels are all short wastes
  the gutter.
- The step strip drops steps that would overflow a narrow frame instead of
  scrolling them.
- `FieldSpan::Half` (two fields sharing a row, as the R2 mock showed) was not
  built — every field takes a full row. The folders page is four rows where two
  would do.
- The remaining prompt callers named in §2.7 are untouched, as scoped.

## 7. Testing and Cleanup

### Tests

Seventy-three net new tests, weighted toward behavior over implementation:

- **Geometry** (7) — rows stack without overlap, buttons right-align uniformly,
  a strip costs exactly one row, and nothing escapes a terminal too small to
  hold the form. That last one found a real defect: a zero-width frame produced
  an inner area at `x = 1`, outside itself.
- **State** (15) — tab order and wrapping, values surviving page switches and
  shape changes, gated creation versus free editing, a select flip adding and
  removing a page, and `Save` from page 3 jumping back to a failure on page 1.
- **Entities and input** (27) — one control per field plus buttons, a closed
  form leaving nothing behind, click-to-focus, a button firing on release and
  *not* when the release drifts off it, unreached steps carrying `UiDisabled`,
  and global bindings not leaking through an open modal.
- **Rendering** (8) — a masked field showing asterisks and never the secret, a
  select showing its label and detail rather than its stored value, and a
  narrow row dropping the detail instead of wrapping it.
- **Wizard** (9) — each provider and auth path end to end against a written
  `config.toml`, prefilled editing, duplicate names refused, and the chain into
  `:set-password`.
- **Key parsing** (4) — `<S-Tab>` and `<BackTab>` compiling to the one
  combination a terminal sends, a plain `<Tab>` left alone, `<C-S-Tab>` keeping
  both modifiers, and the help overlay printing a spelling that parses back.

### Cleanup

Ran the cleanup skill over `overlay/form/`, `accounts/wizard/` and
`ui-kit/src/layer.rs`. The compiler and clippy flagged no dead code, so removal
was driven by grep instead:

- Seven `pub fn`s in `overlay/form/mod.rs` (`submit`, `cancel`, `activate`,
  `move_focus`, `move_cursor`, `next_page`, `prev_page`) had no caller outside
  the module — every one is reached through `dispatch` or from `form::mouse`.
  Narrowed to private so `dispatch` is the honest seam. `SINGLE_PAGE_ID` and
  the `Cursor` re-export were likewise over-exposed.
- Three comments: the select-initial rule was stated in three places and now
  lives only on `resolved_initial`; a `layer::MODAL` doc referencing "the
  confirmations that refactor-ui-v1 will add" lost its forward reference; and
  a doc comment on `enter_on_first_run` that only restated the function name
  was deleted.

Nothing else was removed. `cargo fmt --all` normalized the branch's own files
and touched nothing outside it.
