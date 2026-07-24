# feature - action router - v1

The input backbone: every operation becomes a named command, keys map to command
strings through a per-mode keymap trie (multi-key sequences, chord timeout with
a statusline hint), and `:` opens an aerc-style command line with history and
completion. This is roadmap item 1a.4 — after it lands, all input flows through
one rebindable layer, the hardcoded shell quit bindings are retired, and every
future screen (index, pager, compose, contacts) plugs in by registering commands
and default bindings instead of touching input code.

## 1. Current Design

- **Input today**: the shell (1a.2) binds `q` and `Ctrl-C` directly via plurimus
  `UiActions::key_binding(...).global()` closures that write `AppExit` —
  explicitly temporary, not rebindable, not command-based.
- **Config (1a.3, merged)** provides the raw material: `config::RawKeymaps`
  (context → key-sequence → command string, already syntax-validated at load)
  and `config::parse_key_sequence()` (aerc notation →
  `Vec<crokey::KeyCombination>`), both exported for this item to consume.
  Keymaps default to empty — compiled-in default bindings were deferred to this
  item.
- **plurimus ui feature** (verified in source during 1a.2): a global
  `UiInputBinding::key_passthrough(fn)` delivers every raw `KeyEvent` with
  `&mut World` access — the hook this router needs to receive all keys once,
  without per-widget bindings. `UiActionDisabled` / `UiDisabled` exist for
  suppressing widget-level input.
- **bevy `StatesPlugin`** is already installed (1a.2) but no states are defined.
  bevy 0.18 messages (`Message` derive, `MessageWriter/Reader`,
  `ui_actions_message`) are available.
- **tui-prompts 0.6** is pinned in the workspace, unused — vetted for
  single-line inputs; plurimus's `key_passthrough` is the intended way to drive
  it (per its README example patterns).
- **Statusline** (shell.rs) renders left/right segments from a render-fn state
  struct — it has no slot yet for pending-chord hints or ephemeral error
  messages.
- **Tabs** resource exists (single "mail" tab) — tab-next/prev commands have
  something real to operate on.

## 2. Proposal

Four new modules in the bin crate, one modified:

### `action.rs` — the command vocabulary

- `Action` enum — the single source of truth for operations. Day-one variants:
  `Quit`, `OpenCommandLine`, `TabNext`, `TabPrev`, `Echo(String)`
  (testing/feedback primitive), plus room to grow.
- A command registry: each command = name, argument spec, parse fn → `Action`.
  `parse_command(":tab-next") -> Result<Action>` with unknown-command errors
  naming the input. Commands are the same strings keys.toml binds, so
  keys/command-line/future-macros share one parser.
- `ActionMessage(Action)` bevy `Message`; handler systems consume via
  `MessageReader`: quit → `AppExit`, tab-next/prev → mutate `Tabs`,
  open-command-line → mode switch, echo → statusline message.

### `keymap.rs` — compiled keymaps

- `InputMode` bevy `States`: `Normal` and `CommandLine` now; screens add modes
  later. Keymap **contexts** (keys.toml section names) map onto modes: `global`
  (active in Normal), `command_line` reserved; the full future set (`index`,
  `pager`, `compose`, `contacts`) is accepted as known-but-inactive so configs
  can be written ahead; unknown context names are load errors (strictness
  question R1.1).
- `Keymaps` resource: per-context tries compiled from compiled-in defaults
  overlaid by `RawKeymaps` (user sequence wins; empty command string unbinds).
  Trie nodes keyed by `crokey::KeyCombination` via the existing
  `parse_key_sequence`.
- Compiled-in defaults (global): `q` → `:quit`, `:` → command line, `<Tab>` →
  `:tab-next`, `<S-Tab>`/`<BackTab>` → `:tab-prev`.

### `router.rs` — key resolution

- One persistent router entity carrying a plurimus global `key_passthrough`
  binding that appends every key press to a `PendingKeys` resource (press events
  only; repeats fold in).
- A resolver system per frame: walk the active context's trie with the pending
  buffer — exact match → clear buffer, emit `ActionMessage`; strict prefix of
  longer bindings → keep pending, show the chord in the statusline hint; no
  match → clear with a brief statusline notice.
- **Chord timeout**: pending buffer older than 500ms clears silently (no
  longest-prefix execution — predictability over cleverness).
- In `CommandLine` mode the router yields entirely (keys belong to the prompt)
  except the prompt's own Esc/Enter handling.
- `Ctrl-C` stays a hardcoded emergency exit on the router entity, outside the
  rebindable keymap (R1 question).

### `cmdline.rs` — the `:` command line

- A tui-prompts `TextPrompt` widget entity on the statusline row, spawned
  hidden; `OpenCommandLine` switches `InputMode::CommandLine`, shows it, and
  routes keys to it (plurimus `key_passthrough` targeted at the prompt while its
  mode is active).
- Enter → parse → success: emit `ActionMessage`, close; failure: statusline
  error (themed from the error palette), stay open. Esc → close, discard.
- **History**: up/down cycles; appended to
  `~/.local/state/nitidus/history/commands` (append-only line file per
  persistence.md; loaded at startup, lenient on corruption).
- **Completion**: Tab completes command names (prefix match now; fuzzy via
  nucleo-matcher deferred until pickers need it — R1 question).

### `shell.rs` — integration

- Quit `UiActions` removed from the tab bar entity; router owns input.
- `StatuslineState` gains a **center segment**: pending-chord hint, ephemeral
  messages (`StatusMessage` resource with a time-to-live, styled by severity
  from the theme palettes).

### Testing strategy

Pure units: command parsing (every command, args, unknown/empty errors), keymap
compilation (defaults present, user overlay wins, unbinding, unknown context
rejection), trie walking (exact/prefix/no-match/timeout transitions), history
append/load round-trip, completion candidates. Headless ECS: router app
(ShellPlugin + router plugins, stub key messages injected as plurimus would
deliver them) asserting key→ActionMessage flow, mode transitions, Tabs mutation,
and that `q` still quits via the keymap. pty run for the interactive proof.

Out of scope: screen-specific modes and their command sets (arrive with each
screen), macros/chained commands (Phase 5), fuzzy pickers (nucleo), `:help`
browser (1f.27), mouse bindings in keys.toml, config hot-reload.

## 3. Discussion

### 3.1 R1 Questions

1. **Unknown keymap contexts**: error at load (strict, catches typos like
   `[indx]`) with the future set (`index`, `pager`, `compose`, `contacts`,
   `command_line`) pre-registered as valid-but-inactive — or accept anything
   silently? Proposal: strict with pre-registered set.
2. **Day-one command set**: `:quit`/`:q`, `:tab-next`, `:tab-prev`,
   `:echo <text>` as proposed — anything else you want from the start (e.g.
   `:tab <n>`)?
3. **Ctrl-C**: hardcoded emergency exit outside the keymap (proposed, mutt/aerc
   precedent is roughly this), or rebindable like everything else?
4. **Command history persistence**: append-only file in the state dir from day
   one (proposed), or in-memory until a later polish item?
5. **Completion now**: prefix-only Tab completion (proposed, zero new deps), or
   bring in nucleo-matcher for fuzzy matching immediately?
6. **Chord timeout**: 500ms silent-clear acceptable? And should the no-match
   case show a brief "unbound: gx" statusline notice (proposed) or stay silent?

### 3.2 R1 Answers

1. confirm proposed.
2. confirm
3. keep as is
4. yes
5. bring in necleo
6. proposed

## 4. Plan

Each phase leaves the workspace compiling with clippy and tests green.

### Phase 1 — Commands (`action.rs`)

1. Workspace + bin gain `nucleo-matcher` (R1.5).
2. `Action` enum (`Quit`, `OpenCommandLine`, `TabNext`, `TabPrev`,
   `Echo(String)`), `ActionMessage` bevy Message, command registry
   (`COMMANDS`: name → arg parser), `parse_command()` (accepts with or
   without leading `:`, splits args, unknown/empty errors name the
   input), `complete_command()` (nucleo-matcher fuzzy over registry
   names, best-score ordering).
3. Handler systems (registered by the router plugin in phase 3): quit →
   `AppExit`, tab-next/prev → `Tabs` rotation, echo → status message,
   open-command-line → `NextState<InputMode>`.

### Phase 2 — Keymaps (`keymap.rs`)

4. `InputMode` states (`Normal`, `CommandLine`); `KNOWN_CONTEXTS`
   (`global`, `index`, `pager`, `compose`, `contacts`, `command_line`).
5. `Keymaps` resource: per-context trie (`KeyCombination` edges, command
   string at leaves). `Keymaps::compile(&RawKeymaps)` — compiled-in
   defaults (`q`→`:quit`, `:`→`:command-line`, `<Tab>`→`:tab-next`,
   `<BackTab>`→`:tab-prev`) overlaid by user bindings (user wins, empty
   string unbinds), commands parse-checked at compile, unknown contexts
   are errors. Compilation runs in `run()` right after config load so
   failures exit-early exactly like config errors.
6. `lookup(context, keys) -> KeymapMatch { Exact(Action), Prefix,
   Unbound }`.

### Phase 3 — Router (`router.rs`)

7. `RouterPlugin`: `PendingKeys` resource (buffer + last-press time via
   `Time<Real>`), router entity with a plurimus global
   `key_passthrough` that converts press events to `KeyCombination`s,
   handles the hardcoded `Ctrl-C` exit, ignores keys while in
   `CommandLine` mode, and appends to the buffer.
8. Resolver system (Update, Normal mode): timeout (500ms) clears
   silently; exact → emit + clear; prefix → keep, expose chord hint;
   unbound → clear + status notice ("unbound: gx"). Handler systems from
   phase 1 registered here.

### Phase 4 — Command line (`cmdline.rs`)

9. `CommandLinePlugin`: prompt widget entity on the statusline row
   (tui-prompts `TextPrompt` if its state model cooperates with
   plurimus widget state, else a hand-rolled single-line editor —
   deviation noted in §6 if taken), hidden in Normal mode (statusline
   widget enabled/disabled flags swap on mode change).
10. Key handling in `CommandLine` mode: printable keys edit; Enter
    parses → emit + close, error → themed statusline message + stay
    open; Esc closes; Tab cycles fuzzy completions; Up/Down cycles
    history.
11. History: `state_dir/history/commands` append-only line file; loaded
    leniently at startup, appended on every executed command.

### Phase 5 — Shell integration

12. Remove `quit_actions` from the tab bar; delete `handle_quit`.
13. `StatusMessage` resource (text, severity, expiry) + expiry system;
    statusline center segment renders chord hint or status message
    (severity-styled from theme palettes).

### Phase 6 — Verification

14. fmt/clippy/full suite; pty runs: `q` quits via keymap; `:` opens
    the command line, `echo hi<Enter>` shows `hi` in the statusline;
    `:quit<Enter>` exits; unbound chord shows notice; `Ctrl-C` exits; a
    user keys.toml override rebinding `q` is respected. Record in §5,
    commit per contributing.md.

## 5. Verification

All run 2026-07-24 on rustc/cargo 1.93.1:

- `cargo fmt --check` clean; `cargo clippy --workspace` zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **69 passed, 0 failed**
  (52 bin: command parsing/completion, keymap compile/overlay/unbind/
  unknown-context, trie walking, burst-input end-to-end regressions,
  cmdline editing/history/completion, status expiry, uppercase-key
  normalization; 15 ui-kit; 1+1 mail/contacts).
- pty runs (80×24): `:echo routerworks<Enter>` renders the message in
  the statusline and `q` quits (exit 0); `:quit<Enter>` exits 0; unbound
  `x` shows the `unbound:` notice; `Ctrl-C` exits 0; with a user
  keys.toml (`"Z" = ":quit"`, `"q" = ""`) `q` no longer quits and `Z`
  exits 0.

## 6. Implementation Report

Three design deviations, each forced by a bug the planned design could
not fix, all covered by regression tests:

1. **Plain `Mode` resource instead of bevy `States`.** The pty test
   caught it: burst input (`:quit\r` pasted or typed fast) arrives
   within one frame, but `NextState` applies at the next state
   transition — every key after `:` still routed as Normal mode and the
   whole burst resolved as one unbound blob. Mode switches must be
   synchronous, so `Mode(InputMode)` is a plain resource mutated
   directly; `OnEnter/OnExit` became a `mode.is_changed()` visibility
   system.
2. **Per-key resolution inside the passthrough, not a per-frame
   resolver system.** Same root cause: the resolver must run between
   keys of a burst. `route_key` resolves after every push (buffer only
   ever grows one key deep before resolution), leaving only chord
   timeout and status expiry as frame systems.
3. **Single passthrough; `ActionMessage` machinery removed.** Two global
   passthroughs (router + command line) double-deliver every event with
   entity-order-dependent results — the `:` opening the command line
   would leak into its buffer. The router is now the only passthrough
   and dispatches to `cmdline::handle_key` when the command line owns
   input; actions apply via a direct `apply_action(world, &action)`
   (synchronous, same reason as #1). The message indirection can return
   when a consumer genuinely needs decoupling.

Smaller notes:

- **tui-prompts unused** — the anticipated fallback was taken: a
  hand-rolled ~60-line editor (insert/delete/motion/history/completion)
  avoids its stateful-widget lifetime coupling entirely; the dependency
  stays pinned for future form fields.
- **Command-line errors close the line** and surface as a themed
  statusline error (the row is shared, so stay-open would hide the
  message); plan said stay open — revisit if it feels wrong in use.
- **Uppercase keys**: crokey's parser lowercases its input, so `"Z"` in
  keys.toml silently became `z` while a typed capital arrives
  shift-normalized — bare uppercase letters now canonicalize to
  `shift-<lower>` at parse time (regression-tested; found by pty test E).
- Follow-ups: only the `global` context is consulted (screens will pass
  their own context stack); `Echo` is the sole feedback primitive until
  real screens; chord-hint display in the statusline center works but
  is hard to see at 30fps for fast typists — revisit with the index
  screen.

## 7. Testing and Cleanup
