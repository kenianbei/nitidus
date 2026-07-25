# feature - Contact Book - v1

Roadmap 1e.21, the differentiator phase opens: a real contact book, built fresh
in the `nitidus-contacts` crate (calcard-backed; vcard_tui's design as reference
only, no code imported), living in the app's first non-mail tab — table view,
detail panes, property editors, photos. Autocomplete and address harvesting stay
in 1e.23; import/export and interop polish stay in 1e.22.

## 1. Current Design

- `nitidus-contacts` has been an empty stub since the workspace scaffold
  (`crate_version()` and nothing else). Its manifest declares "contact
  management domain and UI plugins" but it has no dependencies.
- **Tabs exist but are decorative.** `shell::Tabs` holds labels + active index
  and `:tab-next`/`:tab-prev` rotate it, but the label list is `["mail"]` and
  nothing maps the active tab to what the content region shows. What actually
  owns the content region is `Screen` (`Compose | Index | Pager`): every content
  widget draws nothing unless its screen is active, and the router picks the
  Normal-mode keymap context from it.
- UI machinery available to a new screen: plurimus widgets with layout closures
  (`content_layout`, `sidebar_split`, `centered_panel`), the theme resource,
  chained `PromptRequest`s (single-line, masked, prefill — the wizard's `FnOnce`
  chain is the model), and the overlay `PickerSpec` for list choices. The
  statusline's left segment currently always shows mail state (`IndexStatus`
  folder/counts).
- **No vCard machinery in the tree.** `calcard`, `uuid`, `ratatui-image`, and
  `image` are all absent from the workspace manifest.
  [rust-libraries.md](../documentation/rust-libraries.md) §11 settles the
  parser: calcard 0.3.7 (Stalwart) — lenient with the 3.0-isms real exporters
  emit, vCard 4.0 output, CardDAV-ready for phase 5.
- **Persistence is pre-designed** in
  [persistence.md](../documentation/persistence.md) §3: one `.vcf` per contact
  under `~/.local/share/nitidus/contacts/`, UID as filename — the vdir layout,
  khard/vdirsyncer-interoperable — written atomically (tempfile persist in the
  same directory). All contacts held in memory; low volume, no query engine.
  `dirs.rs` resolves config/state/cache but has no data-dir helper yet.
- Compose To/Cc/Bcc are free-text prompts with no completion (1e.23's job), and
  nothing harvests addresses from mail.

## 2. Proposal

1. **Domain model** (`nitidus-contacts`, pure — no bevy): `Contact` wraps a
   calcard `VCard`, exposing typed accessors for the properties the UI handles —
   FN + structured N, EMAIL, TEL, ADR, ORG, TITLE, BDAY, URL, NOTE, PHOTO, UID
   (generated `uuid` v4 for new contacts) — each with its TYPE params
   (home/work/cell). **Properties the UI does not model round-trip untouched**:
   editing one field must never drop or reorder what some other exporter wrote.
   `ContactBook` holds the collection sorted by display name.
2. **Vdir store, minimally pulled forward from 1e.22** — property editors
   without a save path would be theater. `nitidus-contacts` gets load (scan
   `contacts/`, lenient per-file parse: a malformed `.vcf` becomes a startup
   notice naming the file, never a crash), atomic per-contact save on every
   mutation, and file deletion. Import/export commands, khard interop
   verification, and CardDAV-prep sidecars stay in 1e.22. `dirs.rs` gains
   `data_dir()`.
3. **The contacts tab makes tabs real.** `Tabs` gains a `contacts` label and
   `Screen` gains `Contacts`; tab switching now drives the screen — leaving the
   mail tab remembers whether Index or Pager was open and restores it on return.
   While composing, tab switching is refused with a notice (compose stays modal
   until sent/postponed/discarded; compose-as-a-tab is a later spec item). The
   statusline left segment shows contact position/count on the contacts tab
   instead of mail state.
4. **Book UI** (bin crate, `src/contacts/`): the content region splits into a
   table pane (name / primary email / phone / org columns, `j`/`k`/`gg`/`G`
   motions, `/`-less for now) and a detail pane for the selection — properties
   grouped identity-first, every value labeled with its TYPE, unmodeled
   properties listed read-only at the bottom. The photo renders at the top of
   the detail pane. Focus toggles between panes (`Tab`, mirroring the mail
   sidebar contract).
5. **Property editors** on the detail pane: `e` edits the highlighted property
   through a chained prompt (structured values like N and ADR walk their
   components; Enter keeps the prefilled current value), `a` adds one (picker of
   property kinds → TYPE picker where applicable → value prompt), `x` removes
   the highlighted property, `n` creates a contact (name → email chain, then
   lands in the detail pane to flesh out), `D` deletes the contact behind a y/n
   confirm. Every mutation saves through the atomic writer immediately — the
   file is the undo horizon (git-friendly per the persistence doc).
6. **Photos**: PHOTO (inline base64 or local file URI) decoded via `image`,
   rendered with `ratatui-image`'s auto-negotiated protocol (kitty/iTerm2/
   sixel, halfblock fallback) — degradable: no photo or no graphics support
   shows a placeholder, never an error.
7. **Commands and keys**: `:contacts` jumps to the tab from anywhere
   (`:tab-next` still rotates); new `contacts` keymap context with the bindings
   above plus `?` help integration.

Out of scope: import/export and interop testing (1e.22), autocomplete,
harvesting, `:add-contact`-from-sender and `:compose-to` bridges (1e.23),
CardDAV sync (phase 5), and contact search/filtering (with 1f.24's machinery).

## 3. Discussion

### 3.1 R1 Questions

1. **Persistence pull-forward.** The roadmap puts persistence in 1e.22, but
   editors need a save path, so the proposal pulls load + atomic save + delete
   into this feature and leaves import/export + interop polish for 1e.22. Agree
   with that split, or would you rather 1e.21 be read-only over a hand-seeded
   vdir and all writes wait for 1e.22?
2. **Crate split.** Proposal: `nitidus-contacts` stays pure domain + vdir store
   (calcard, uuid, tempfile — no bevy), and the tab UI lives in the bin crate
   (`src/contacts/`) where the prompt/overlay/theme machinery already is —
   despite the stub manifest promising "UI plugins" in `nitidus-contacts`. The
   alternative (UI plugin inside `nitidus-contacts`) means moving or
   re-exporting the bin crate's prompt and overlay machinery. Recommend the pure
   split; confirm?
3. **Photos now or later.** Photos are in the roadmap line, but they carry the
   two heaviest new dependencies (`ratatui-image`, `image`) and
   terminal-protocol negotiation. Proposal keeps them, built last and fully
   degradable. Fine — or defer photos to a follow-up and land the book
   text-only?
4. **Tab semantics.** Confirm: tab switch drives `Screen`, mail tab restores
   Index/Pager as last left, compose refuses tab switches with a notice until
   closed. (The spec's "tabs as universal container" with compose as a tab stays
   future work.)
5. **Three-pane reading.** The spec says "table view, 3-pane detail view".
   Proposed interpretation: table pane + detail pane, with the photo panel as
   the detail pane's third region. If vcard_tui's actual layout differs in a way
   you liked (e.g. list | detail | photo as three side-by-side columns), say so
   and I'll match it.
6. **Editor property set.** First-class editors for FN/N, EMAIL, TEL, ADR, ORG,
   TITLE, BDAY, URL, NOTE (+ TYPE params); everything else preserved and shown
   read-only. Anything you want added to (or cut from) the first-class set?
7. **Smoke plan.** Headless pty smoke over a seeded vdir (a handful of fixture
   contacts, one with a photo, one with exotic unmodeled properties): open the
   tab, navigate, edit a phone, add an email, create and delete a contact,
   verify the `.vcf` files (including round-trip preservation) with a diff. A
   khard cross-read is 1e.22's interop job. OK?

### 3.2 R1 Answers

1. agreed
2. agreed
3. photos now
4. agreed, and take a look at comfy-tabs if it will work with our ui mouse and
   keyboard hover/focus system. If not give recommendation.
5. yes
6. yes, though we could provide a custom field where advanced users can edit the
   property string directly, and we validate the property. What do you think?
7. Let me smoke test instead, pty can take a long time and uses token credits.

### 3.3 R2 Questions

1. **comfy-tabs: it works — recommend adopting it here.** `ratatui-comfy-tabs`
   0.5.12 (2026-07, active, GitLab `comfyhome`) depends only on `ratatui-core`
   0.1.2 + `unicode-width`, so it slots straight into our ratatui 0.30 stack. It
   is a `StatefulWidget` (`TabNav` + `TabNavState`), which is exactly what
   `Widget::from_render_fn_with_state` wraps, and — the part your question
   hinged on — its interaction API is abstract: keyboard is
   `select_direction(...)` calls we drive from the existing
   `:tab-next`/`:tab-prev` actions, and all mouse handlers
   (`handle_mouse_click/wheel/reorder_*`) take plain `column: u16, row: u16`
   against the widget's `Rect`, no crossterm coupling. That is precisely the
   shape plurimus's hover/press markers can feed when the 1f.27 mouse pass
   arrives; nothing conflicts, and until then we simply don't call the mouse
   methods. Two costs to accept: (a) it renders tabs as individually bordered
   boxes, so the tab strip grows from the current 1 row to ~3 rows of chrome (a
   `split_shell` layout change); (b) it is young and single-maintainer — though
   the surface we'd use is small enough to replace with our Paragraph bar again
   in an afternoon. Adopt it in this feature?
2. **Raw property editor: yes — include it.** Proposed as `E` on the highlighted
   property (including the otherwise read-only unmodeled ones): prompt prefilled
   with the property's serialized vCard line (`EMAIL;TYPE=work:nk@example.com`),
   on submit the line is validated by round-tripping it through calcard inside a
   minimal vCard wrapper; a line that doesn't yield exactly one well-formed
   property re-prompts with the parse error. One honesty note: calcard is
   deliberately lenient (Postel's law), so validation catches structural garbage
   but will accept some nonsense values — strictness beyond "parses as a
   property" isn't available without hand-rolling RFC 6350 value grammars. Good
   enough?
3. **Smoke handoff (transcribing #7).** No pty smoke; I seed a fixture vdir
   (including a photo contact and an exotic-properties contact), hand you a
   short checklist, and you drive the live smoke. The `.vcf` round-trip diff
   check I can still do headlessly after your run — it's just file comparison,
   no pty.

### 3.4 R2 Answers

1. adopt
2. yes
3. agreed

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **Domain + vdir store** (`nitidus-contacts` grows its body): calcard,
   uuid, and tempfile dependencies; `Contact` wrapping a calcard `VCard`
   with typed accessors (FN/N, EMAIL, TEL, ADR, ORG, TITLE, BDAY, URL,
   NOTE, PHOTO, UID + TYPE params) that mutate in place so unmodeled
   properties round-trip untouched; `ContactBook` sorted by display
   name; vdir store (lenient directory load reporting per-file errors,
   atomic tempfile-persist save, delete). `dirs.rs` gains `data_dir()`.
   Unit tests: accessor edits, round-trip preservation of exotic
   properties, atomic save, malformed-file reporting, uid filenames.
2. **Tabs become real.** `ratatui-comfy-tabs` renders the tab strip
   (`TabNav`/`TabNavState` behind the shell widget; `split_shell` grows
   the strip to the boxes' height); `Screen::Contacts` + a `contacts`
   tab label; tab switching drives `Screen` (mail tab remembers
   Index/Pager and restores; compose refuses with a notice);
   `:contacts` jumps by name; statusline left segment switches to
   contact state on the contacts tab. The contacts screen is an empty
   placeholder. Tests: rotate drives screen both ways, restore
   behavior, compose refusal.
3. **Book UI, read-only.** `src/contacts/` plugin in the bin crate:
   startup load into a `ContactBookResource` (bad files become
   notices), table pane (name/email/phone/org) + detail pane
   (identity-first groups, TYPE labels, unmodeled properties read-only
   at the bottom), pane focus toggle, motions, `contacts` keymap
   context, `?` help rows. Tests: navigation, detail selection,
   statusline counts over a temp vdir.
4. **Editors and mutations.** `e` edit chain (structured N/ADR walk
   components), `a` add (kind picker → TYPE picker → value), `x`
   remove property, `E` raw property line with calcard round-trip
   validation, `n` new contact (name → email), `D` delete with y/n
   confirm; every mutation saves atomically and re-sorts the book.
   Tests: each verb end-to-end over a temp vdir with file-level
   assertions, including preservation diffs and raw-editor rejection.
5. **Photos.** `image` + `ratatui-image` dependencies; PHOTO decode
   (inline base64 or file URI) into a detail-pane thumbnail with
   auto-negotiated protocol and halfblock fallback; missing photo or
   unsupported terminal degrades to a placeholder. Decode helpers unit
   tested; rendering exercised by the manual smoke.
6. **Verification & smoke handoff.** Clippy + full workspace test run
   with counts; seed a fixture vdir (photo contact, exotic-properties
   contact) and hand Norman a smoke checklist; after his pass, headless
   `.vcf` round-trip diff verification. Fill §5–§7.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **354 passed, 0
  failed** (was 319 at branch start).
- New coverage:
  - domain (19 unit tests): accessor edits, round-trip preservation of
    exotic properties through an edit, UID immutability (edit, remove,
    and sneak-in-via-add all rejected), raw-line validation, book
    sorting/upsert, atomic vdir save/load/delete, foreign-filename
    preservation (no orphaned copies), filename sanitization,
    malformed-file reporting;
  - shell: tab switches drive `Screen` and restore the mail screen,
    named activation, compose refusal with a notice, layout heights;
  - contact book e2e (10 tests over a seeded temp vdir): sorted load,
    table/detail navigation with pane focus, detail-cursor reset on
    selection change, `e` edit reaching disk with TYPE and exotic
    properties intact, `a` add flow through both pickers, `E` raw
    editor rejecting a UID swap and re-prompting, `x` property
    removal, `n` create (file + selection), `D` delete with
    decline-keeps, startup notices for malformed files;
  - photos: inline base64 and `file://` decode, remote URL refusal,
    no-photo fallback.
- Live smoke (Norman, per §3.3-3): **PASSED** — fixture vdir seeded at
  `~/.local/share/nitidus/contacts/` (Ada: full property set + inline
  photo; Grace: exotic unmodeled properties; Mel: minimal; plus a
  deliberately broken `.vcf`); startup notice, tab strip, photo
  rendering, both editors, add/remove/create/delete, and the
  pager-restore + compose-refusal tab semantics all as expected.
- Post-smoke round-trip diff against pristine copies: untouched files
  byte-identical; rewritten files differ only by calcard's writer
  normalizations (TYPE params uppercased, long-line folding, a
  `base64\,` comma escape in the PHOTO data URI that re-parses to the
  same binary — probed) — **except one real bug it exposed, fixed
  below** (§7).

## 6. Implementation Report

- **Domain honesty through one door**: every mutation — typed editors
  included — becomes a single vCard line re-parsed by calcard before it
  touches the card, so edits are validated exactly like loaded files
  and unmodeled entries are never rebuilt (they round-trip untouched,
  proven by test). UID is immutable through every path; deleting FN is
  allowed (display falls back to empty, sorting to UID).
- **Vdir pragmatics**: filenames are `{uid}.vcf`, but a foreign file
  whose name is not its UID keeps its filename on save — no orphaned
  duplicates when pointing nitidus at an existing khard directory.
  Missing-UID cards get a generated one injected at load.
- **Tabs are real now**: the comfy-tabs strip costs 2 extra chrome rows
  (`TAB_BAR_HEIGHT` 1 → 3); tab state drives `Screen`, with
  `MailScreenMemory` restoring Index/Pager and compose refusing
  switches. `<Tab>` inside the contacts tab toggles pane focus
  (mirroring the index's sidebar-focus shadowing), so switching back to
  mail is `<BackTab>`, `:contacts`/`:tab-next`, or a future `gt`.
- **Photos**: protocol negotiation runs before bevy_ratatui owns stdio
  (the query does its own raw-mode dance and would race the input
  reader afterwards); headless or unsupported terminals degrade to the
  PHOTO detail row's `[N bytes]` text. Decoded protocols are cached per
  contact UID; remote photo URLs are never fetched (pager's
  no-remote-content stance). Thumbnail disk caching
  (`~/.cache/nitidus/photos` per persistence.md) is deferred until
  decode cost ever matters at real book sizes.
- **Editors**: `e` on N/ADR walks components with prefills; other
  modeled properties get a single value prompt in raw vCard value
  syntax; unmodeled rows point at `E`, which edits any property as a
  full line and re-prompts with the reason on rejection. `a` chains
  kind picker → TYPE picker → value (address walks components);
  values from single prompts are escaped, so a comma in a NOTE cannot
  smuggle syntax.
- Follow-ups: `:compose-to`/`:add-contact` bridges and autocomplete
  (1e.23), import/export + khard interop verification (1e.22), photo
  thumbnail disk cache, contact search (with 1f.24), a `gt`-style
  mail-tab jump key if `<BackTab>` proves annoying, and comfy-tabs
  mouse hookup when the 1f.27 mouse pass lands.

## 7. Testing and Cleanup

- **Bug caught by the round-trip diff, fixed**: Ada's
  `N:Lovelace;Ada;Augusta;Countess;` came back as `N:Lovelace;;;;`.
  calcard represents a compound value as one `Text` value *per
  component* (`Component` only appears for comma-lists inside one),
  but `components_of` read only the first value — so the N/ADR edit
  chains prefilled just the first component and Enter-through wiped
  the rest. Fixed to flatten all values; regression test
  (`n_edit_prefills_every_component_so_enter_through_preserves_them`)
  proven red on the old code, green on the fix.
- Interop observation for 1e.22: calcard's writer escapes the comma in
  `PHOTO:data:...;base64\,...` and uppercases TYPE params; both
  re-parse identically (probed against the smoke-rewritten file), but
  the khard cross-read planned for 1e.22 should watch that escape.
- Cleanup scope: the branch diff vs main. Comments state invariants
  (round-trip contract, UID immutability, save-before-upsert, the
  stdio-race reason for early photo negotiation); no dead code —
  clippy silent, every helper has callers. `render.rs` was split
  (`draw.rs`) when it crossed the 300-line budget. Smoke artifacts
  removed (`broken-fixture.vcf`, the `contacts.pristine/` snapshot);
  the three fixture contacts remain in the live vdir.
- Final verification after the smoke and fix:
  `cargo clippy --workspace --all-targets` zero warnings;
  `CARGO_INCREMENTAL=0 cargo test --workspace` **355 passed, 0
  failed** (suite counts confirmed present).
