# refactor - Keymap Modernization - v1

Requested during 1f.25's discussion (now parked on its branch pending this):
realign the default keybindings with the modern-TUI conventions of yazi,
lazygit, and helix — with the stated caveat that they are slightly different
apps. Everything-is-a-command stays; this changes only which commands the
compiled-in defaults bind. `keys.toml` overrides keep working unchanged.
Behavior change = the rebindings themselves, listed exhaustively in §2.

## 1. Current Design

Defaults live in `keymap/defaults.rs` as per-context tables (global, index,
pager, picker, sidebar, compose, contacts), compiled into the trie with
`keys.toml` overlaid. Today's layout grew binding-by-binding across features:

- **Global**: `q` quit, `:` command line, `<Tab>`/`<BackTab>` tab next/prev —
  but `<Tab>` is shadowed in every major context (sidebar focus in index/pager,
  pane focus in contacts, help-scope in the picker), so tab switching actually
  happens via `<BackTab>` or `:contacts`.
- **Index**: `j`/`k`/arrows/pages, `gg`/`G`, `Enter` view, `u` toggle-read, `F`
  toggle-flag, `T` threads, `za`/`zM`/`zR` folds, `P` parent, `b` sidebar,
  `<Tab>` sidebar focus, `m` compose, `z` undo-send, `r`/`R`/`f`
  reply/reply-all/forward, `e` recall, `d` delete, `A` add-contact, `l`
  limit-prefill, `/` `n` `N` search.
- **Pager**: `Esc` close, `j`/`k`/`Space`/pages scroll, `J`/`K` next/prev
  message, `H` headers, `S` skip-quoted, `]`/`[` parts, `s`/`o` save/open part,
  `l` links, plus the shared mail verbs.
- **Contacts**: motions, `<Tab>`/`Enter` pane focus, `e`/`E`/`a`/`x` property
  editors, `n` new, `D` delete, `P` photo, `m` mail-to.
- No marking/batch keys exist yet (1f.25 is parked on exactly this).

What the three reference apps agree on, from their docs:

- **Selection-first bulk ops** (yazi, helix, lazygit's range select): `<Space>`
  toggles selection (yazi advances the cursor), `v` enters a visual/range mode,
  `Esc` universally cancels/clears.
- **`d`/`D` = trash / permanent delete** (yazi) — exactly our delete semantics,
  currently spelled "d, and d again inside the trash".
- **`[`/`]` for tab/view switching** (yazi tabs, lazygit panel tabs) plus yazi's
  `1-9` direct tab jumps; `<Tab>` is never tab-switching in any of the three —
  it is local focus/info movement, which is what nitidus's contexts already want
  it for.
- **`/` search with `n`/`N`** (all three; landed in 1f.24).
- **`,` as a sort prefix** (yazi: `,m` modified, `,a` alphabetical …).
- **`z` undo** (lazygit) — 1f.25's plan already.
- **`Esc` = back/reset**, `?` = help, `:` = commands, `q` = quit — all three,
  already ours.
- helix's `Space`-leader menu is noted and deliberately **not** adopted now: it
  needs a which-key-style hint UI to be worth anything; recorded as a phase 2
  candidate alongside selectable keymap schemes.

## 2. Proposal

Adopt the shared conventions; keep mail-identity keys where the reference apps'
meanings would fight forty years of mail muscle memory (`f` stays forward, not
yazi's filter — the deviation the caveat anticipated).

**Global (all contexts):**

| Key                 | Was                          | Becomes                                                                                          |
| ------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------ |
| `[` / `]`           | —                            | tab prev / tab next                                                                              |
| `1` / `2`           | —                            | jump to mail / contacts tab                                                                      |
| `<Tab>`/`<BackTab>` | tab next/prev                | **removed from global** — `<Tab>` is now purely the local focus key each context already made it |
| `q`, `:`, `?`       | quit, command, help(context) | unchanged (`?` promoted global)                                                                  |

**Index:**

| Key                      | Was         | Becomes                                                                                                                   |
| ------------------------ | ----------- | ------------------------------------------------------------------------------------------------------------------------- |
| `<Space>`                | —           | toggle mark + advance _(lands with 1f.25)_                                                                                |
| `v`                      | —           | visual range _(1f.25)_                                                                                                    |
| `t`                      | —           | mark thread _(1f.25)_                                                                                                     |
| `Esc`                    | —           | clear marks / visual _(1f.25)_                                                                                            |
| `z`                      | undo-send   | `:undo` (staged ops, then send) _(1f.25)_                                                                                 |
| `D`                      | —           | permanent delete with confirm, any folder (yazi pair; `d` keeps trash semantics, and inside trash `d` still confirms)     |
| `*`                      | —           | toggle flag (star)                                                                                                        |
| `F`                      | toggle flag | **unbound** (freed)                                                                                                       |
| `,d` `,f` `,s` `,u` `,F` | —           | sort by date/from/subject/unread/flagged (yazi's sort prefix; `,r` reverses, `,,` resets)                                 |
| everything else          |             | unchanged (`j/k`, `gg/G`, `Enter`, `u`, `T`, folds, `b`, `<Tab>` sidebar focus, `m`, `r/R/f`, `e`, `d`, `A`, `l`, `/n N`) |

**Pager:**

| Key             | Was            | Becomes                        |
| --------------- | -------------- | ------------------------------ |
| `{` / `}`       | —              | prev / next MIME part          |
| `[` / `]`       | prev/next part | freed for global tab switching |
| everything else |                | unchanged                      |

**Contacts / sidebar / picker / compose:** unchanged — their `<Tab>` focus keys
now agree with the global layout instead of shadowing it.

Mechanically: `defaults.rs` tables, a handful of new command entries (`:tab N`
jump, `:sort` already exists — the `,` prefix binds to it, `:delete-permanent`
for `D`), and the help table picks everything up for free. `keys.toml` users are
unaffected except where they relied on the removed global `<Tab>`.

Out of scope: the 1f.25 marking keys themselves (that feature implements them;
this doc reserves them), helix-style leader menus and which-key hints,
selectable schemes (phase 2), and any pager/compose verb redesign.

## 3. Discussion

### 3.1 R1 Questions

1. **The core adoptions.** `[`/`]` + `1`/`2` for tabs (freeing `<Tab>` to be
   purely local focus), `d`/`D` trash/permanent, `,`-prefix sorts,
   `Space`/`v`/`t`/`Esc`/`z` reserved for 1f.25 marking/undo. Confirm?
2. **Flag's new home.** `F` frees up (it collides with nothing, but `*` is the
   star mnemonic and keeps `F` available for a future yazi-style filter).
   Proposed `*`; alternatives: keep `F`, or `s` (Gmail star — but pager uses `s`
   for save-part). Your call.
3. **`f` stays forward.** The one deliberate deviation from yazi (`f` = filter
   there); `l` keeps limit-prefill. Confirm?
4. **Pager parts on `{`/`}`.** Frees `[`/`]` for uniform tab switching
   everywhere. Confirm?
5. **`D` permanent-delete everywhere** (confirmed y/n prompt, staged + undoable
   once 1f.25 lands) — include now, or keep permanent delete trash-only?
6. **Smoke.** Quick manual pass over the new bindings (tabs via `[`/`]` and
   `1`/`2`, `*` flag, `,` sorts, `{`/`}` parts, `D` confirm) — you drive as
   usual?

### 3.2 R1 Answers

1. confirmed
2. proposed \*
3. confirmed
4. yep
5. include now
6. yep

### 3.3 R2 (from chat)

Norman asked for yazi-style left/right pane navigation, and whether a
persistent preview pane (index | preview miller columns) is small or large.
Assessment: the arrows are small (bindings + a focus dispatch over existing
state); the preview pane is a large architectural feature (selection-driven
auto-fetch with debounce/cancellation, `Screen` semantics, width budgets) —
**split off** into a stub UI-refactor doc (`refactor-ui-v1.md`, committed as
a stub to be designed later; it also claims better overlay/toast use for
confirms and errors). Decision: add arrow navigation to this refactor now:

| Context | `<Left>` | `<Right>` |
| --- | --- | --- |
| index | focus the sidebar (showing it if hidden) | open the selection |
| sidebar | — | focus back to the index |
| pager | close (the Esc reflex, yazi's "out") | — |
| contacts | focus the table pane | focus the detail pane |

One dispatching pair (`:focus-left`/`:focus-right`) resolves per context;
`h`/`l` deliberately not used (`l` is limit since 1f.24).

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **New verbs the bindings need.** `:tab <n>` (positional jump),
   `:delete-permanent` (the existing permanent-confirm path, callable
   from any folder, pager's open message included), and
   `:sort-reverse` (toggle the current sort's direction — `,r`).
   Actions + command table entries + unit coverage.
2. **The rebinding.** `defaults.rs`: global gains `[`/`]`, `1`/`2`,
   and `?`; loses `<Tab>`/`<BackTab>`. Index: `F` → `*`, `D`
   permanent, the `,` sort family (`,d ,f ,s ,u ,F ,r ,,`). Pager:
   parts `]`/`[` → `}`/`{`, `D` bound. Per-context `?` duplicates
   removed in favor of the global. Affected tests updated (the
   Tab-shadows-tab-next router test's premise is gone).
3. **E2e coverage of the new layout.** `[`/`]`/`1`/`2` drive the tab
   from the index, `,d`/`,r` sort, `*` flags, `D` prompts the
   permanent confirm outside the trash.
4. **Arrow pane navigation (R2).** `:focus-left`/`:focus-right`
   dispatch over screen + sidebar/contacts focus state; `<Left>` in
   the pager closes. Bound in index, sidebar, pager, and contacts
   contexts. E2e tests for the focus transitions.
5. **Verification & smoke handoff.** Clippy + full run with counts;
   Norman's pass over tabs, `*`, `,` sorts, `{`/`}` parts, `D`, and
   the arrows. Fill §5–§7.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **392 passed, 0
  failed** (was 387 at branch start).
- New coverage (`tests/keymap_layout.rs`): `[`/`]` and `1`/`2` drive
  the tab and the screen, the `,` sort family sets key/reverse and
  `,,` resets, `*` flags the selection, `D` opens the permanent
  confirm outside the trash, and the arrows walk sidebar ↔ index and
  the contact panes.
- Updated tests: the help view now expects `]`:tab-next as an
  unshadowed global; the router's Tab test asserts "local focus,
  never tab switching" (its old premise — global Tab being shadowed —
  is gone by design).
- Live smoke (Norman): **PASSED** — bracket/number tab switching with
  Tab staying local focus, `*` flag, `,` sorts, `{`/`}` parts, `D`
  confirm, and the arrow navigation. One bug caught and fixed during
  the smoke: sidebar `→` only returned focus instead of opening the
  folder under the cursor — it now behaves exactly like Enter
  (regression-tested with a real folder row through the router).

## 6. Implementation Report

- The rebinding itself was table edits; the substance was three small
  verbs the layout needed: `:tab <n>` (positional jump through the
  same compose-refusing tab machinery), `:delete-permanent` (the
  existing confirmed-purge path made reachable from any folder —
  pager's open message included), and `:sort-reverse` (flip direction
  without touching the key, for `,r`).
- `?` promoted to a true global removed five per-context duplicates;
  the help table derives from bindings, so it followed automatically.
- Behavior notes: `<BackTab>` no longer does anything by default;
  `keys.toml` users who bound around the old global `<Tab>` are
  unaffected (their overrides still compile in on top).
- Deviations kept deliberately: `f` = forward (not yazi's filter),
  `<Space>` in the pager stays page-down (reading position), picker
  `<Tab>` stays help-scope.
- Arrow navigation (R2) landed as one dispatching command pair; the
  pager's `<Left>`-closes matches the future preview-pane direction
  recorded in the `refactor-ui-v1` stub (committed alongside, to be
  designed later — preview pane, overlay confirms, toast errors).
- 1f.25 resumes on the reserved keys: `<Space>`/`v`/`t`/`Esc` marking
  and `z` `:undo` — its design doc's key questions are now settled.
- Follow-ups: helix-style leader menu + which-key hints and
  selectable schemes stay phase 2; a future yazi-style `F` filter is
  open now that `F` is unbound.

## 7. Testing and Cleanup

- Cleanup scope: the branch diff vs main. Comments state invariants
  (Tab-is-local-focus, the yazi out/in reflex, why `f` stays forward);
  no dead code — clippy silent. The stale router test name from the
  old shadowing world was renamed to what it now proves.
- The `refactor-ui-v1` stub rides along into main deliberately
  (agreed in chat): scope claimed, design deferred.
- Final verification after the smoke:
  `cargo clippy --workspace --all-targets` zero warnings;
  `CARGO_INCREMENTAL=0 cargo test --workspace` **392 passed, 0
  failed** (suite counts confirmed present).
