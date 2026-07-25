# feature - Composer - v1

Roadmap item 1c.14, the first of phase 1c. Composing a new message: prompted
headers, body written in `$EDITOR` (the TUI suspends, mail sync continues), and
a review screen with the compose keybinding cheat-sheet. The composer produces a
finished compose session; actually building and transmitting the RFC 5322
message is 1c.15 (send pipeline), replies are 1c.16, and
drafts/attachments/recovery are 1c.17.

## 1. Current Design

What exists to build on:

- `Screen { Index, Pager }` gates router context and widget visibility; the
  `compose` keymap context is already reserved in `KNOWN_CONTEXTS`. The pager
  shows the pattern a third screen follows (main-column widget, `active` flag,
  inactive-draws-nothing).
- The command line is a single-line editor (insert/delete/motion, history,
  completion) hard-wired to command execution — nothing else can prompt for a
  line of input today. tui-prompts sits unused in the workspace deps.
- `AccountConfig` carries `email`, `display_name`, `aliases`, `signature`,
  `signature_file`, and `Folders { drafts, sent, … }` — identity and signature
  are config-ready.
- Suspend/resume verified against the deps: bevy_ratatui's `RatatuiContext`
  restores the terminal on drop and re-enters raw mode + alternate screen on
  `init()`; crossterm events are polled by a bevy system (nothing reads the tty
  while the frame loop is blocked); plurimus draws every widget every frame
  through ratatui's buffer diff, so a fresh `RatatuiContext` after the editor
  exits repaints the whole UI without any invalidation machinery. The engine
  runs on its own tokio runtime — sync, IDLE, and the cache writer continue
  while the main thread waits on the editor.
- The help overlay enumerates any context's bindings with summaries — a compose
  cheat-sheet can be generated, not hand-written.

## 2. Proposal

### 2.1 Prompt line (generalized command line)

The command-line machinery grows a second mode: a `Prompt` carrying a label
(`To: `), an initial value, and an `on_submit` closure — same widget, same
editing keys, same router gate; Esc cancels back to the session's review stage.
Command execution becomes just the default prompt. This is deliberately _not_ a
multi-field form; tui-prompts stays unused until a real multi-field dialog
exists.

### 2.2 The compose session

`ComposeSession` resource (one at a time):

- `account` + `from` identity resolved from the account config.
- Headers: `to`, `cc`, `bcc`, `subject` (strings; address validation and
  autocomplete land with 1e.23).
- `body_path`: a file under `state_dir/compose/<epoch>-<pid>.md`, created at
  session start with the signature appended (`signature` config key, else
  `signature_file` contents, separated by `-- `). The file outlives crashes by
  construction; 1c.17 adds recall.
- Stage machine: `PromptTo → PromptSubject → Editing → Review`, with
  review-initiated excursions back into prompts (`PromptHeader`) or the editor,
  and a `ConfirmDiscard` prompt on quit.

Entry: `m` in the index (`:compose` everywhere). Flow is mutt-shaped: To prompt
→ Subject prompt → editor → review.

### 2.3 Editor suspend/resume

An exclusive-world action: remove the `RatatuiContext` resource (its drop
restores the terminal), run `$VISUAL` → `$EDITOR` → `vi` on `body_path` blocking
the main thread, then insert a fresh `RatatuiContext::init()` — the next frame
repaints everything. A non-zero editor exit keeps the session and surfaces a
statusline warning. Mail sync continues throughout (engine-owned runtime).

### 2.4 Review screen

`Screen::Compose`, a main-column widget in the pager's mold:

- Headers block (From/To/Cc/Bcc/Subject; empty optional headers dimmed), a body
  preview (read-only, scrollable with the shared motions), and a **cheat-sheet
  footer built from the live compose keymap** (sequence + summary, wrapped to
  width) — rebindings show up automatically, and `?` opens the full help
  overlay.
- Compose-context bindings: `e` edit body, `t`/`c`/`b`/`s` re-prompt
  To/Cc/Bcc/Subject, `y` send, `P` postpone, `q`/`Esc` discard (via a `y/n`
  confirm prompt; discard deletes the body file).
- `y` and `P` are **stubs in this item**: statusline notices naming 1c.15/1c.17.
  The session survives, so nothing is lost by pressing them early.

### 2.5 Wiring

- `Action::{Compose, ComposeOp(...)}` + commands with summaries; `compose`
  context defaults; `Screen::Compose` in the router match and `dispatch_motion`
  (review-body scrolling).
- The sidebar stays visible beside the review (same rule as the pager); folder
  switching or `view` during compose is blocked with a notice rather than
  silently dropping the session.
- Tests: prompt-line unit tests, session state-machine unit tests, integration
  tests driving `m` → prompts → (editor stubbed by injecting a body) → review
  render + header re-prompt + discard. The real `$EDITOR` round-trip is
  pty-smoked (`EDITOR=` a script that appends a line and exits).

## 3. Discussion

### 3.1 R1 Questions

1. **Flow shape.** `m` → `To:` prompt → `Subject:` prompt → `$EDITOR` → review
   screen (mutt-shaped; Esc at either prompt aborts cleanly). Confirm, or would
   you rather land on the review screen first and fill everything from there?
2. **Prompt line over tui-prompts.** Header input reuses the command-line editor
   as a labeled prompt with a submit callback — one field at a time, no
   multi-field form yet. tui-prompts waits for a real form (account wizard,
   1d.20). OK?
3. **Send/postpone as stubs.** `y` and `P` surface "lands with 1c.15/1c.17"
   notices and keep the session. Acceptable for this item, or should `y` be
   hidden entirely until the pipeline exists?
4. **Editor choice.** `$VISUAL` → `$EDITOR` → `vi`, no config key yet (config
   gains one when the settings batching lands). Body file kept under
   `state_dir/compose/` for crash survival. Confirm?
5. **Discard semantics.** `q` asks `Discard message? (y/n)` via the prompt line;
   `y` deletes the body file and returns to the index. Postponing (keeping the
   file) is 1c.17's recall. OK?
6. **Cheat-sheet.** Generated from the live compose keymap (so keys.toml rebinds
   reflect); rendered as a two-or-three-line footer, with `?` for the full
   overlay. Confirm?

### 3.2 R1 Answers

1. That looks good
2. Not sure what you mean here, you're not going to use tui-prompts? Also I
   think tui-prompt has some basic validation adornments?
3. stubs are fine.
4. I think we should explore possible inline text editors, otherwise yes, we can
   use `$VISUAL` → `$EDITOR` → `vi`.
5. Any reason we aren't using escape for return? I feel like q should be a
   dedicated hotkey for quit application.
6. yes.

### 3.3 R2 Notes

1. **tui-prompts: yes, using it (R1-2).** The hesitation was only about
   multi-field *forms*; for the single prompt line tui-prompts is the
   right tool and the spec's pick. `TextPrompt`/`TextState` render the
   label, wrap the value, and carry a `Status` (pending/done/aborted)
   adornment usable for basic validation (e.g. To must be non-empty).
   Keys still flow through our router — the prompt handler feeds the
   `TextState` directly, so the router-owns-input architecture holds.
   The command line keeps its bespoke editor (history + completion are
   custom); prompts are a new `Prompt` input mode beside it.
2. **Inline editors explored (R1-4).** tui-textarea 0.7 (the natural
   fit) pins ratatui 0.29 — incompatible with our 0.30 workspace; edtui
   0.11 interops in principle via the ratatui-core split but drags
   syntect, clipboard/X11, and image dependencies for a compose box.
   Decision: `$VISUAL` → `$EDITOR` → `vi` now; an inline body editor
   becomes a follow-up when tui-textarea supports ratatui 0.30
   (tracked in §6).
3. **Esc leaves, q quits (R1-5).** Compose binds `Esc` to the discard
   confirm and does not bind `q` at all — `q` stays the dedicated
   global quit. For consistency this item also rebinds the pager's
   close from `q` to `Esc`, so `q` quits the app uniformly everywhere.
   Note: pressing `q` with a staged message quits the app; the body
   file survives on disk by design (1c.17 adds recall), only unsent
   headers are lost.

## 4. Plan

Each phase leaves the workspace compiling, clippy-clean, and tests green.

**Phase 1 — prompt mode on tui-prompts.** `InputMode` gains
`Prompt`; new `prompt.rs`: `PromptRequest { label, initial, on_submit:
Box<dyn FnOnce(&mut World, String)>, on_cancel: Box<dyn FnOnce(&mut World)> }`,
a `PromptState` resource wrapping a tui-prompts `TextState`, a
statusline-row widget (visibility swapped exactly like the command
line's), and a router gate ahead of the Normal path: printable keys
feed the `TextState`, Enter submits, Esc cancels. Unit tests for the
editing round-trip and submit/cancel dispatch.

**Phase 2 — compose session.** `compose/mod.rs`: `ComposeSession`
resource (account, from identity, to/cc/bcc/subject, `body_path`,
stage), created by `Action::Compose` (`m` in the index, `:compose`)
with the body file under `state_dir/compose/` carrying the account
signature. Prompt chaining To → Subject via Phase 1 (Esc mid-chain
discards cleanly). Unit tests for session creation, signature
selection, and stage transitions.

**Phase 3 — editor suspend/resume.** Exclusive action in
`compose/editor.rs`: drop `RatatuiContext` (restores the terminal),
run `$VISUAL` → `$EDITOR` → `vi` on the body file (blocking; engine
sync unaffected), insert a fresh `RatatuiContext::init()`; non-zero
exit warns and keeps the session. Enters automatically after the
Subject prompt and from `e` on review.

**Phase 4 — review screen.** `Screen::Compose` + `compose/render.rs`
widget in the main column (pager pattern): headers block, scrollable
body preview (shared motions via `dispatch_motion`), cheat-sheet
footer generated from the live compose keymap rows. Compose bindings:
`e` edit body, `t`/`c`/`b`/`s` re-prompt headers, `y`/`P` stub
notices, `Esc` discard confirm (deletes the body file). Pager close
rebinds `q` → `Esc`. Guards: `:compose`/`:view` during an active
session surface notices. Integration tests: `m` → prompts → (body
injected, editor bypassed) → review rows, header re-prompt, discard;
pager-Esc regression.

**Phase 5 — smoke + docs.** Pty smoke with `EDITOR` set to a script
appending a line: full `m` → To → Subject → editor → review round
trip over the live corpus, cheat-sheet visible, Esc-y discard.
Record §5/§6.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **238 passed, 0 failed**
  (was 228 pre-feature: +2 prompt-line tests, +3 session unit tests,
  +5 compose integration tests).
- Integration coverage: the m → To → Subject → editor → review chain
  (editor stubbed via the `EditorCommand` override appending to the
  body), signature presence, header re-prompts starting from current
  values, discard confirm (n keeps, y deletes the body file and
  returns to the index), Esc mid-chain cleanup, and m resuming an
  existing session.
- Pty smoke over the live corpus with `VISUAL` set to a script
  appending a line: full compose round trip — To/Subject prompts on
  the statusline row, the terminal suspended into the fake editor and
  restored cleanly, review screen beside the sidebar showing
  From/To/Cc `(none)`/Bcc `(none)`/Subject, the editor's line in the
  body, and the generated cheat-sheet footer.

## 6. Implementation Report

Implemented per plan, with these findings and deviations:

- **Browsing while composing is allowed, not blocked** (deviation from
  §2.5): the review screen carries `Tab`/`b` sidebar bindings, folder
  switching works normally, and `m` resumes the staged session — this
  fell out simpler than blocking and is friendlier; `:compose` while
  staged resumes instead of erroring.
- **Headless editor safety:** the suspend path skips the terminal
  teardown/re-init when no `RatatuiContext` exists, and an
  `EditorCommand` resource overrides `$VISUAL`/`$EDITOR` so tests
  never mutate process env (test-isolation rule).
- The cheat-sheet filters motion bindings (j/k/arrows are universal
  noise); `?` still lists everything. tui-prompts drives the prompt
  line exactly as planned — labels, editing, Status-based
  Enter/Esc — with the router feeding keys.
- The body file gained a guaranteed trailing newline (an appending
  editor otherwise glues onto the signature) — caught by the
  integration test.
- `command/table.rs` crossed 300 lines with the nine compose commands;
  they moved to `command/compose_table.rs`, which 1c.15–17 will grow.
- Follow-ups: inline body editor when tui-textarea supports ratatui
  0.30; address validation + autocomplete (1e.23); an editor config
  key with the settings batch; quit-with-staged-session confirmation
  (currently `q` quits and only the body file survives).

## 7. Testing and Cleanup
