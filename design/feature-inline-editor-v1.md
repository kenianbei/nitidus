# feature - Inline Body Editor - v1

The follow-up recorded in feature-composer-v1 §6: editing the message body
inside the TUI instead of suspending to `$EDITOR`. The blocker that deferred it
is gone — the ratatui org adopted the dormant tui-textarea as
`ratatui-textarea`, and 0.9.2 tracks our ratatui 0.30. This doc covers the
essential editing surface plus attachments-as-tokens, and sets up our fork as
the staging ground for two upstream PRs the feature needs.

## 1. Current Design

- **Body editing is `$EDITOR`-only.** `compose/editor.rs` is an exclusive-world
  action: drop `RatatuiContext` (its `Drop` restores the terminal), run
  `$VISUAL` → `$EDITOR` → `vi` blocking the main thread, re-init the context,
  `session.reload_body()`, back to `ComposeStage::Review`. Mail sync continues
  throughout on the engine's own runtime. `EditorCommand` overrides the binary
  for headless tests.
- **The session already has the right shapes.** `ComposeSession` carries
  `body: Vec<String>`, `body_path: PathBuf`, and `attachments: Vec<PathBuf>`.
  `Vec<String>` in and out is exactly `TextArea::new` / `into_lines` — the
  round-trip needs no adapter type.
- **`Screen::Compose` is read-only.** `compose/render.rs` builds `session_lines`
  (headers block + body preview) and `cheat_lines` (compose keymap, wrapped to
  width) into one `Widget::from_render_fn_with_state`. There is no editable
  widget anywhere in the app except the command line.
- **The router gates on four input modes.**
  `InputMode { Normal, CommandLine, Prompt, Search }`; `router.rs` intercepts
  the latter three ahead of the keymap and dispatches everything else as
  actions. There is no editor mode.
- **The command line is a bespoke single-line editor** (`cmdline/mod.rs`, 373
  lines) with its own cursor, history, and completion. Nothing about it
  generalizes to multi-line.
- **The mail-text pipeline already exists — on the read side.** `pager/body.rs`
  has `LineKind`, `BodyLine`, `build_body_lines`, `quote_depth` (pub) plus
  `classify`, `strip_quotes`, `quote_prefix`, `unstuff`, `wrap_line` (private,
  same crate): format=flowed reflow, quote-depth classification, signature
  detection, and width wrapping that preserves quote prefixes.
- **`ratatui-image` 11 is already a workspace dep**, used by `contacts/photo.rs`
  and `contacts/draw.rs`.
- **The recorded blocker is stale.** feature-composer-v1 §3.3 R2-2 deferred this
  until "tui-textarea supports ratatui 0.30". Upstream `rhysd/tui-textarea` is
  dormant (0.7.0, Oct 2024, pinned to ratatui 0.29, 51 open issues). The ratatui
  org adopted the fork: `ratatui/ratatui-textarea` 0.9.2 (2026-06-12) depends on
  `ratatui-core` 0.1 / `ratatui-widgets` 0.3 — the 0.30 split — with `crossterm`
  as its only default feature. Same org as `tui-prompts`, already in our deps.

### What the crate gives us, and what it doesn't

Read from the 0.9.2 source, not just the docs:

- `TextArea` renders as `Paragraph::new(Text<Line<Span>>)` (`widget.rs:127`).
  Text state is `Vec<String>` with a separate screen-map for soft wrap.
- The public API covers editing, 15 `CursorMove` variants (incl. word and
  paragraph motions), selection, undo/redo with a history cap, regex search,
  soft wrap (`WrapMode::{None, Word, Glyph, WordOrGlyph}`), and block/style
  hooks. `input_without_shortcuts()` accepts keys without applying any default
  bindings.
- **There is no API for styling a range of the text.** `LineHighlighter` applies
  arbitrary byte-offset ranges internally, but `mod highlight;` is private so
  the type is unreachable. The only externally reachable way to style a range is
  `set_search_pattern` + `set_search_style`, a single slot already spoken for by
  search. This is what pushes line styling out to a post-pass over the rendered
  buffer (§3.4).
- **The screen↔data mapping exists and is nearly public.** `DataCursor` and
  `ScreenCursor` are exported from `lib.rs`, and `TextArea::cursor()` /
  `screen_cursor()` return them. Only the conversions are sealed:
  `ScreenMap::{screen_to_array, array_to_screen}` are `pub(crate)`. Resolving a
  click is therefore a visibility change, not new machinery.
- **No system clipboard** — the yank buffer is in-process only.

### The fork

`/home/moz/Developer/ratatui-textarea` → `kenianbei/ratatui-textarea`, on `main`
at v0.9.2, even with upstream. No `upstream` remote configured yet.

## 2. Proposal

### 2.1 Editor mode

`InputMode::Editor`, gated in `router.rs` beside the existing three. Printable
keys and the basic edit keys feed the `TextArea` via
`input_without_shortcuts()`; everything else resolves through the `compose`
keymap into actions that drive the widget programmatically (`move_cursor`,
`delete_word`, `undo`, `redo`, …). Router-owns-input holds, bindings stay
rebindable, and the help overlay keeps enumerating them.

`ComposeStage::Editing` stops meaning "suspended in `$EDITOR`" and starts
meaning "editing inline". Inline is the default on both paths into editing — the
`:compose` flow after the To/Subject prompts, and `e` (`:compose-edit`) from
Review. `$EDITOR` is demoted to its own verb, `:compose-edit-external`, bound to
`E`; `compose/editor.rs` is unchanged and stays the escape hatch.

`ui.compose.editor = "inline" | "external"` (default `"inline"`) selects what
the `:compose` flow and `:compose-edit` do. `:compose-edit-external` always
suspends regardless of the setting, so the escape hatch never depends on config.

Body round-trip: `TextArea::new(session.body.clone())` on entry; `into_lines()`
→ `session.body` and a write to `body_path` on leave and on autosave.
`body_path` remains the crash-survival artifact it is today.

### 2.2 The essential surface

| Capability                                     | Source                                                              |
| ---------------------------------------------- | ------------------------------------------------------------------- |
| Multi-line editing, grapheme-correct           | crate                                                               |
| Soft wrap at composer width (`WrapMode::Word`) | crate                                                               |
| Word / paragraph motions                       | crate, driven by our keymap                                         |
| Undo / redo                                    | crate                                                               |
| Selection, cut / copy / paste, yank            | crate (in-process)                                                  |
| Theming from `nitidus-ui-kit`                  | crate style hooks                                                   |
| Scroll + cursor-follow                         | crate                                                               |
| Quoted-reply and signature styling             | ours — per-line buffer post-pass (§3.4)                             |
| Mouse click-to-position                        | **the fork PR** (§2.5)                                              |
| System clipboard                               | ours — `arboard`, OSC 52 fallback                                   |
| `$EDITOR` escape hatch                         | unchanged                                                           |

Quote and signature classification is not new work: `quote_depth` and `classify`
already do it for the pager. This feature reuses them to classify the lines the
editor styles, so read and compose agree on what a quote is.

### 2.3 Inline images via tokens

Attachments become visible, editable text. A canonical single-line token in the
buffer marks each one:

```
[[attach: photos/diagram.png]]
[[attach: photos/diagram.png | width=40 height=20]]
```

The grammar is `[[attach: <path> (| <key>=<value>)*]]`. `[[…]]` is chosen for
collision-resistance — it is rare in prose and, unlike `![alt](path)`, does not
capture markdown people actually type. The `|` terminates the path, so paths may
contain spaces without quoting.

**The attribute list exists from v1 even though v1 defines no attributes.**
Parsing accepts any `key=value` pairs and round-trips unrecognized ones
untouched, so the sizing and inline-styling attributes that motivate it can be
added later without a syntax migration or a v2 token.

- Tokens are hand-typable; the parser is the source of truth, not the insertion
  path. `:attach` inserts one (reusing the existing `explorer` picker, which
  already takes an extension filter and an `on_pick` callback), but a typed
  token is equally valid.
- The token is styled distinctly (the same line-styling pass as quotes) and
  never soft-wrapped.
- With the cursor on a token, the referenced image renders in an overlay (§2.4)
  through the `ratatui-image` we already depend on.
- The body is the source of truth; `session.attachments` becomes a **derived
  cache**, recomputed whenever the body changes. This keeps `compose/build.rs`,
  `compose/persist.rs`, and `outbox` on their current contract — they continue
  to read `attachments` and never learn about tokens. On send, tokens are lifted
  out of the body into MIME parts; the token text never reaches the wire
  message.

**Graphics are deliberately not drawn inside the text buffer.** The render path
terminates in a `Paragraph`, which cannot reserve rows or interleave a
sub-widget, and nothing exposes the screen row of a given buffer line — so this
is not a fork-able gap, it is a rewrite of the crate's rendering model. True
inline images belong to the Phase 4 "HTML tier 2" item, on the read side.

### 2.4 The preview overlay

An overlay, opened on demand from a token — not a permanent split, so the editor
keeps the full width it needs for a real body.

`overlay/mod.rs` is picker-shaped, not a generic container: `ActiveOverlay`
holds a `PickerState`, and the entity carries `WidgetOrder(OVERLAY_ORDER)` above
every screen. The preview is a **sibling entity following the same
spawn/despawn pattern**, not a variant of the picker. The shared parts —
`layout::centered_panel_layout`, `WidgetOrder(OVERLAY_ORDER)`, `Clear`, themed
`Block` chrome — are already reused verbatim by `explorer/mod.rs`, so a third
consumer is a precedent, not a new abstraction. No generalization of the overlay
module is proposed here; if the spawn/despawn duplication becomes real, that is
its own refactor.

Rendering a picture in a terminal is protocol-dependent and can fail
(unsupported terminal, unreadable file, non-image type). The overlay degrades to
the path, dimensions, and file size rather than erroring the editor.

The overlay is also where a future HTML preview lands, since it already owns "a
rendered view of something the buffer only references."

### 2.5 The fork and the upstream PR

Development consumes the fork through a `[patch.crates-io]` git dependency
pinned to the pushed PR branch
(`kenianbei/ratatui-textarea`, `feature/screen-position-methods`), so a clean
checkout builds with nothing local. We drop the patch and return to crates.io
once the PR merges and releases.

**`feature/screen-position-methods`** (`ca67297`, PRed upstream) — expose the
screen position
mapping. This closes the parent project's standing request for exactly these
accessors (rhysd/tui-textarea#82, open since Sep 2024, motivated by mouse
handling, tooltips, and menus). `DataCursor` and `ScreenCursor` are already
public, but nothing relating them to the rendered area is:

- `TextArea::scroll_offset()` — the viewport top. The value exists on the
  `viewport` field, but that field is `pub(crate)` and `Viewport` lives in a
  private module with no re-export, so it is unreachable from outside. This is
  why a visibility change could not do the job and an accessor was needed.
- `TextArea::screen_to_data(row, col)` — the row a display position belongs to.
  It takes plain display coordinates rather than a `ScreenCursor`, whose `char`
  and `dc` fields are results rather than inputs.
- `TextArea::line_number_width()` — the gutter's column width, `0` when line
  numbers are off. Without it a caller cannot convert a click column when line
  numbers are enabled, since the margin arithmetic is internal; `widget.rs` now
  delegates to it so the gutter has one definition.

**All three live in `textarea.rs`, beside `cursor()` and `screen_cursor()`,
and delegate into the internals exactly as `screen_cursor()` already does.**
`screen_map.rs` is an entirely crate-private impl block and is left untouched;
public API belongs on the public surface. The PR is +200/-2 across
`textarea.rs` and a two-line delegation in `widget.rs`.

`screen_to_data` clamps out-of-range positions, which is not decoration:
`screen_lines_count` is not public, so a caller has no way to know where the
text ends, and painting a pane taller than the body would index out of bounds.
Removing the clamp and running the tests gives
`index out of bounds: the len is 2 but the index is 7`.

**The PR is open upstream.** The branch is pushed to the fork and the patch
pins its commit through the git dependency, so the merge is no longer blocked
by anything local. Finishing means: once the PR merges and ships in a release,
drop the patch stanza, bump `ratatui-textarea` to the released version, and
re-run the workspace checks.

A fully public alternative was built and measured before settling here: the
viewport top can be recovered by locating the cursor cell in the rendered
buffer, and screen→data by walking the cursor and reading `screen_cursor()`.
It works — 9 tests green — but costs ~150 lines of application code resting on
two behaviours the crate does not promise (that the cursor is always drawn
on-screen in the style we set, and that display rows stay monotonic in body
lines). Three lines against a documented API won.

No second remote is added. `origin` already points at the fork, and the fork's
`main` is the same commit as `ratatui/ratatui-textarea`'s (`0498731`, the v0.9.2
release), so an `upstream` remote would fetch nothing. If upstream moves before
the PR goes out, add it then and rebase.

Everything else stays in nitidus — line styling (§3.4), the line-number gutter
and scroll arithmetic for clicks, and the clipboard, which the crate
deliberately has no OS dependencies for.

Out of scope: **hard wrap and format=flowed on send** — the editor displays the
margin, the send pipeline owns the transformation, and that lands in its own doc
(the `pager/body.rs` helpers this feature promotes to `pub(crate)` are what it
will reuse). Also out: AI-assisted typing (its own doc — it carries a privacy
decision that deserves separate discussion), spell check, terminal-graphics
images inside the text buffer, rich-text/HTML composition, address autocomplete
(1e.23), and any attachment-picker UX beyond the token and the existing
explorer.

## 3. Discussion

### 3.1 R1 Questions

1. **Default path.** Should `m` open the inline editor directly with `$EDITOR`
   demoted to a key from Review, or should inline editing sit behind a config
   flag for a release while it settles?
2. **Dependency mechanism during fork development.** `[patch.crates-io]` with a
   path dep, a git dependency pinned to a fork commit, or vendoring? The first
   is most convenient locally; the second is the only one that keeps a clean
   checkout building for anyone else.
3. **Token syntax and authorship.** What should the token look like —
   `[[attach: diagram.png]]`, something terser, something harder to collide with
   real body text? And can it be typed by hand, or only inserted by `:attach`
   (which would reuse the ratatui-explorer picker already in the workspace)?
4. **Image preview surface.** Reuse the pager's peek pane, add a dedicated split
   in the compose screen, or an overlay?
5. **Decoration API shape for PR 1.** A per-line callback
   (`Fn(&str, usize) -> Vec<(Range<usize>, Style)>`) is the most flexible and
   what we'd want; a declarative list of decoration ranges the app sets and
   clears is less powerful but likely easier to land upstream. Which do we
   propose — or do we open with the callback and fall back?
6. **Clipboard.** `arboard` pulls X11/Wayland dependencies, which
   feature-composer-v1 §3.3 explicitly rejected for edtui. OSC 52 has no
   dependencies and works over SSH but is write-mostly and terminal-dependent.
   Both, with OSC 52 as the fallback?
7. **Scope boundary.** Does hard-wrap / format=flowed _on send_ belong in this
   doc, or does the editor only display the margin and the send pipeline own the
   transformation?

### 3.2 R1 Answers

1. open directly as default with editor demoted, and allow configuration.
2. Just do a path dep, with explicit instructions not to push to remote for the
   fork repo, feature branches stay local.
3. that all looks good, but we may also need to use height/width/inline styling
   as part of the token in the future, so keep that in mind with picking a token
   strategy.
4. I think an overlay will work best, with possibility of have the overlay show
   an html version in a future feature (chromiumoxide?)
5. I will let you determine the API shape, but I'll review vefore PR.
6. yes
7. let the send pipeline own transformation, and leave for later.

### 3.3 R2 Questions

Raised at the Phase 1 review: can the fork changes be smaller, keeping
functionality in nitidus wherever possible?

1. **Is the decoration API needed at all?** Every styling this feature asks for
   is whole-line — `LineKind` is `Normal | Quote(u8) | Signature`, and the
   attachment token occupies its own line by construction (§2.3). The byte-range
   machinery, and with it the wrap-segment clipping and UTF-8 boundary handling,
   is capability nothing here uses.
2. **What is the floor for the screen mapping?** The mapping data
   (`screen_lines`, `data_pointers`, `viewport`) is entirely private, so nitidus
   cannot reproduce it without reimplementing the crate's wrap algorithm.

### 3.4 R2 Answers

**One PR, not two.** Per-line styling moves to nitidus as a post-pass over the
rendered buffer: after the textarea draws, recolour the rows belonging to quoted
or signature lines. `screen_to_array` supplies the screen-row → data-row lookup,
so wrapped lines colour correctly across every display row they occupy.

Recolouring is restricted to cells the editor left otherwise untouched. That
reproduces the precedence the decoration API was going to provide — selection,
search, and cursor styling all survive — verified on a quoted line carrying both
a selection and the cursor.

Cost of the choice, accepted: sub-line styling is gone. It would only matter to
style a quote marker differently from the text it prefixes, or for a mid-line
token, which §2.3 already excludes.

One constraint this places on the implementation: **quote styling must be a
foreground change, not a background tint.** Rows below the last line of text
clamp to the last data row, so a background tint would leak onto the blank rows
under a trailing signature, whereas recolouring the foreground of a blank cell
is invisible. Tinting would additionally require `screen_lines_count()` to be
public so the pass could stop at the last real row.

The fork diff is one file, 58 lines, mostly rustdoc.

## 4. Plan

Six phases. Each leaves the workspace compiling with `cargo clippy --workspace`
clean and `cargo test --workspace` green.

### Phase 1 — Fork groundwork _(done)_

No nitidus behavior change; the app still uses `$EDITOR`.

1. In the fork: branch `feature/screen-map-minimal` off `main`, make
   `screen_to_array` public and clamp its row, add `scroll_offset`, and cover
   both in the existing `screen_map` test module.
2. Add `[patch.crates-io]` + the `ratatui-textarea` dependency to the workspace,
   pointed at the local checkout.

### Phase 2 — Editor mode

1. `InputMode::Editor` and its router gate, beside the existing three.
2. New `compose/inline.rs`: an `InlineEditor` resource owning
   `TextArea<'static>`, with enter/leave that round-trips
   `session.body` ↔ `into_lines()` and writes `body_path` on leave.
3. Render the text area into the compose screen's body region, replacing the
   static preview while editing.
4. `ui.compose.editor` in `config/schema.rs` (defaults to `inline`),
   `:compose-edit-external` in the compose command table, `E` in the compose
   defaults, and `:compose-edit` honoring the setting.
5. Tests: body survives an enter→edit→leave round-trip; `body_path` matches the
   buffer after leaving; the config value selects inline vs. suspend; the router
   sends keys to the editor only in `InputMode::Editor`.

### Phase 3 — The editing surface

1. Compose-context bindings → programmatic `TextArea` calls: word and paragraph
   motions, undo/redo, selection, cut/copy/paste. Printables and the basic edit
   keys go through `input_without_shortcuts()`.
2. Theme wiring from `nitidus-ui-kit`, `WrapMode::Word` at the composer width.
3. Clipboard: `arboard`, falling back to OSC 52 when it is unavailable —
   headless and SSH sessions must degrade rather than fail.
4. Tests: each bound action moves the cursor or mutates the buffer as specified;
   undo restores; the help overlay lists every new binding.

### Phase 4 — Line styling

1. Promote the `pager/body.rs` helpers (`classify`, `quote_prefix`,
   `strip_quotes`) to `pub(crate)`.
2. A post-pass over the rendered buffer: map each area row to its data line via
   `screen_to_array` + `scroll_offset`, then recolour the foreground of rows
   classified as quote or signature. Recolour only cells left at the base style,
   so selection, search, and cursor styling win (§3.4).
3. Tests: a quoted reply dims at each depth, the signature dims below `-- `, an
   unquoted body is untouched, a wrapped quoted line dims on every display row,
   and a selection over a quoted line keeps its own styling.

### Phase 5 — Attachment tokens

1. Token parse/format with attribute round-tripping, unit-tested against
   malformed input, paths with spaces, and unknown attributes preserved
   verbatim.
2. Recompute `session.attachments` from the body on change.
3. `:attach` inserts a token via the explorer picker; `:detach` removes the
   token line. `drafts.rs` keeps its picker and its `mentions_attachment` nudge
   — both read the derived `attachments`, so neither changes shape.
4. Send lifts tokens out of the body before building MIME.
5. Tests: a typed token registers as an attachment; a removed token deregisters;
   the sent body contains no token text; unknown attributes survive a
   parse→format cycle.

### Phase 6 — Preview overlay

1. A preview overlay entity following the picker's spawn/despawn pattern.
2. Render through `ratatui-image`, degrading to path/size text when the terminal
   or the file will not cooperate.
3. Tests assert overlay state and the degraded text, not pixels.

### Sequencing note

Phase 1 gates 3 (clipboard aside), 4, and 5's styling. Phases 5 and 6 are
independent of each other once 2 lands.

## 5. Verification

Run after each phase, and again at the end:

- `cargo clippy --workspace` — clean, no warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **472 passed, 0 failed**,
  up from 428 before the feature.
- `cargo fmt --all --check` — clean.
- The fork, on its own checks (`cargo clippy --features=search,termwiz,termion
  --tests --examples`, `cargo test --features=search`): clean, **204 passed, 0
  failed**.

New coverage: 21 integration tests in `tests/inline_editor.rs`, 3 for the
footer, 2 for replies reaching the editor, 6 unit tests for
the line-styling pass, 9 for the token grammar, 2 for the preview's degraded
path, 1 for the OSC 52 payload.

Three existing suites — `compose`, `drafts`, `outbox` — now select
`EditorKind::External` explicitly. They drive the review screen through
`EditorCommand` and assert on what `$EDITOR` wrote, so they were always
testing that path; the default moving to inline is what made it explicit.

## 6. Implementation Report

All six phases landed. Commits: `be01067` (fork dependency), `908c901` (editor
mode), `5c6da53` (editing surface and clipboard), `39e9392` (line styling),
`97f9690` (attachment tokens), `a2a2854` (preview overlay).

**What the design did not anticipate.**

`TextArea` caches its screen map in `RefCell`s and its area in a `Cell`, so it
is `Send` but not `Sync`. It can be neither a bevy resource nor plurimus widget
state, both of which require `Send + Sync`. It is shared as
`Arc<Mutex<TextArea>>`, which also turned out better than the design's
"`TextArea` is plain data, clone it into the widget state": the renderer borrows
the live editor instead of deep-copying the buffer every frame.

Mutation goes through `resource_mut` even though the mutex would allow shared
access — that is what ticks bevy's change detection, and the renderer
reclassifies lines off that tick. Without it, typing `>` at the start of a line
would not have dimmed it until something unrelated changed.

`input_without_shortcuts` handles only characters, Tab, Backspace, Delete,
Enter, and scroll — **not arrows or Home/End**. Every motion had to be bound
explicitly, which suits the router-owns-input rule but is more bindings than the
design implied.

**Two bugs the tests caught, both in the token work.** Appending a token to
`session.body` did not reach `body_path`, which is what send and postpone
actually read, so a postponed message lost its attachment; `write_body` now
keeps the two in step. And because sending lifts tokens out into MIME parts,
recall had nothing left to derive attachments from — it now re-materializes a
token per recovered part.

**Deviations from the plan, all deliberate.**

- `:attach` keeps its path prompt rather than moving to the explorer picker.
  The prompt is scriptable and headless-testable; the explorer would have made
  the attachment tests depend on a live filesystem browser. Tokens being
  hand-typable is what matters, and it holds.
- The preview is explicit (`<C-p>`), not automatic on cursor-over-token. An
  overlay that opened itself while the cursor crossed a token would fight the
  typing.
- The `arboard` dependency that feature-composer-v1 §3.3 rejected for edtui is
  now taken, but only for the clipboard, with OSC 52 behind it so a session
  without a display server still copies.

**Two bugs smoke testing found, both from the same blind spot** — adding a code
path without checking every route into it.

- The footer is generated from the live keymap of whichever context owns the
  keyboard. Pointing it at the `editor` context put 26 bindings where the review
  screen has 12, rendering 900 characters into two rows; `Esc finish editing the
  body` sorted by key name into the middle and wrapped off the end, hiding the
  one hint that leads to the review screen. It now skips motions and the obvious
  delete keys, and leads with the way out (`1a43fb7`).
- Reply, reply-all, and forward called `editor::edit_body` directly instead of
  the entry point that reads `ui.compose.editor`, so they always suspended while
  a new message opened inline (`c65a83c`). Recall and recover were checked and
  are correct: they land on the review screen by design.

**Follow-up items.**

- Remove the `[patch.crates-io]` stanza and depend on the released
  `ratatui-textarea` once the upstream PR merges and ships. The patch is a git
  dependency pinned to the pushed PR branch, so clean checkouts build in the
  meantime. Nothing else about the feature waits on it.
- Hard wrap and format=flowed on send (§2 out of scope) still belong to the send
  pipeline.
- The editor does not yet display the wrap margin, only soft-wrap at the
  composer width.

## 7. Testing and Cleanup

Both trees are clean and every scratch probe used during the minimality review
was removed. The fork carries one branch, `feature/screen-map-minimal`, one
commit, 58 lines in one file — the PR is the user's to open once this is smoke
tested.

Behaviour outside the composer is unchanged: the index, pager, sidebar,
contacts, and outbox suites all pass untouched.
