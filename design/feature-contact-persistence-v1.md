# feature - Contact Persistence - v1

Roadmap 1e.22, right-sized by history: the vdir layout and atomic writes it
names already shipped inside 1e.21 (editors needed a save path). What remains is
the way in and out — `:import-contacts` / `:export-contacts` over standard
multi-card `.vcf` files — plus the khard interoperability verification the
contact-book doc deferred here, including its flagged writer-normalization
observation.

## 1. Current Design

- `nitidus-contacts` already persists: `load_dir` (lenient per-file parse,
  issues reported), `save_contact` (atomic tempfile-persist, `{uid}.vcf`,
  foreign filenames kept), `delete_contact`. Missing-UID cards get a generated
  UID injected at load. Every mutation in the contact tab saves before the
  in-memory book updates.
- **There is no way in or out of the vdir from inside nitidus.** Getting an
  existing address book in means copying files by hand; getting one out means
  the file manager. The standard interchange shape — one `.vcf` containing many
  vCards, which is what Google/Outlook/Apple exports produce — is not readable
  at all: `Contact::from_vcf` parses the **first** card of its input and ignores
  the rest (calcard's `Parser::entry()` is an iterator; we call it once).
- Command-line machinery for argument commands exists (`named_arg`, e.g.
  `:move <folder>`, `:folder-create <name>`); argument completion is a recorded
  follow-up (command names only today). Nothing in the app expands `~` in
  user-typed paths; the pager's attachment save writes into a fixed `SaveDir`
  rather than prompting for paths.
- **Interop status**: unverified. 1e.21's post-smoke diff recorded calcard
  writer normalizations that other tools will see on any card nitidus rewrites:
  TYPE parameters uppercased, long-line folding, and the comma in
  `PHOTO:data:...;base64,` escaped to `base64\,` (re-parses identically in
  calcard; other parsers' tolerance unknown). khard and vdirsyncer are not
  installed on this machine.
- CardDAV sync metadata (ETags, tokens, tombstones) is designed to live in
  `mail.db` per [persistence.md](../documentation/persistence.md) §3 — a phase 5
  concern, nothing to prepare now.

## 2. Proposal

1. **Multi-card parsing in the domain**: `nitidus-contacts` gains
   `parse_all(input)` — iterate calcard's parser to `Eof`, yielding every vCard
   as a `Contact` (UID injection as at load) plus a list of per-card issues for
   anything malformed (Postel: bad cards are reported and skipped, good cards
   proceed).
2. **`:import-contacts <path>`**: read the file, `parse_all`, then for each card
   — **skip when the UID already exists in the book** (import must never clobber
   local edits), otherwise atomic-save into the vdir and upsert into the book.
   Statusline summary: `imported 12, skipped 3 existing, 1 failed`; per-card
   failure details to the log. A directory path imports every `.vcf` inside it
   (the "copy a vdir in" case), same rules.
3. **`:export-contacts <path>`**: serialize the whole book as vCard 4.0 into one
   `.vcf` at the given path — atomic write, refuses to overwrite an existing
   file (statusline says so; delete it first if you mean it). Exports are the
   book as nitidus holds it (calcard's normalized output).
4. **Path UX**: both commands expand a leading `~`; relative paths resolve
   against the working directory. Errors (unreadable, unparseable, already
   exists) land on the statusline. Argument path completion stays a recorded
   follow-up with the command-line's completion work.
5. **khard interop verification** (the 1e.22 deliverable that is a test, not
   code): point khard at the live vdir in a throwaway Python venv, verify it
   lists and shows nitidus-written contacts — specifically a card nitidus has
   rewritten (uppercased TYPEs, folded lines, `base64\,` PHOTO escape) — and
   that a khard-edited card loads back into nitidus with its edits intact.
   Findings recorded in §5; any real incompatibility becomes a bugfix doc.

Out of scope: CardDAV (phase 5, metadata design already settled), autocomplete
and the `:add-contact`/`:compose-to` bridges (1e.23), per-contact selective
export (a follow-up if wanted), and import from formats other than vCard.

## 3. Discussion

### 3.1 R1 Questions

1. **Import duplicate policy.** Proposal: skip cards whose UID already exists in
   the book, with counts in the statusline — importing never overwrites local
   data. Alternatives: overwrite (destructive), or import-as-new with a fresh
   UID (duplicates people). Confirm skip?
2. **Export shape.** One command, whole book, single multi-card `.vcf`,
   refuse-to-overwrite. Per-contact export (e.g. export the selection only)
   deferred unless you want it now. Confirm?
3. **Directory import.** Worth keeping the "path may be a directory of `.vcf`s"
   case (it makes migrating an existing khard/vdirsyncer collection one
   command), or restrict v1 to single files?
4. **khard verification method.** khard isn't installed; proposal is a throwaway
   venv (`python -m venv`, `pip install khard`, minimal config pointed at the
   live vdir), removed afterwards. Alternative: `pacman -S khard` if you'd
   rather have it around for real use — your call, and it changes nothing about
   the checks.
5. **Smoke.** Import/export and the khard checks are all headless and
   file-verifiable — I'd run everything myself and skip a manual smoke entirely
   this time. Fine, or do you want to drive an import of a real exported `.vcf`
   (e.g. from Google Takeout) yourself?

### 3.2 R1 Answers

1. skip
2. whole book
3. single files for v1
4. why do you need khard? I'd prefer not to use it. We are storing in the db,
   correct?
5. I can smoke test

For file import, do we have a file picker already? We could use one of these:

- https://github.com/sorinirimies/tui-file-explorer
- https://github.com/tatounee/ratatui-explorer

Any preference?

Also, we could use this for photo selection. How will photos be stored with the
vcard? Inline or as a path? Does neomutt or aerc or abook address this issue?

### 3.3 R2 Questions

1. **Where contacts actually live (answering §3.2-4's question).** Not the db —
   contacts are one `.vcf` file per contact in
   `~/.local/share/nitidus/contacts/`, per
   [persistence.md](../documentation/persistence.md) §3: _files are the source
   of truth for contacts_; `mail.db` only caches mail and is deletable. khard
   was never a storage dependency — it was only a verification tool to prove
   third-party software can read our files. Since you don't use it: **dropped**.
   Interop confidence comes from the round-trip tests instead, and if a real
   interop report ever arrives it becomes a bugfix doc. (§2.5 is superseded.)
2. **File picker: recommend `ratatui-explorer`.** 0.3.0 (2026-03, 128k
   downloads) depends on `ratatui ^0.30` + `educe` only, and its API slots into
   our modal pattern: `handle(impl Into<Input>)` takes crossterm events the
   router can forward exactly like the picker overlay, `current()` gives the
   selection, `set_filter_map` restricts to `.vcf`, themable. The alternative
   `tui-file-explorer` (1.1.11, young) hauls in `ratatui-image ^10` —
   conflicting with our 11, so two copies in the tree — plus serde_json, libc,
   and tui-slider. Proposed use: `:import-contacts` with no argument opens the
   explorer (Enter on a `.vcf` imports it); the path argument form stays for
   scripting. Export keeps a plain path — an explorer can't pick a file that
   doesn't exist yet, so `:export-contacts` with no argument opens a prompt
   prefilled with `~/nitidus-contacts.vcf`. Confirm?
3. **Photos: inline, downscaled — and a `:set-photo` verb riding along.** vCard
   supports both inline base64 and a URI/path; nitidus already _reads_ both. For
   writing, inline is the right default: it survives file moves, machine syncs,
   and future CardDAV, and it's what Google/Apple exports do. Path-based PHOTO
   breaks the moment the image file moves and never roams. Prior art is thin —
   neomutt, aerc, and abook have no contact photos at all (text-only address
   books); GUI clients all embed. To bound file growth, embed a downscaled JPEG
   (max edge 256px, ~15–30 KB) rather than the original. Proposal:
   `P`/`:set-photo` on the contact detail opens the same file explorer filtered
   to images, decodes + downscales + embeds; removing a photo is already `x` on
   the PHOTO row. This grows 1e.22 by one editor verb, justified by the picker
   synergy. Include?

### 3.4 R2 Answers

1. got it... does it make sense to store contacts in the db? How do we plan for
   the eventual possibility of syncing contacts with google, outlook, etc? If
   vcards are only stored as a file this may not make sense? Thoughts?
2. confirm
3. yes.

### 3.5 R3 — files vs db, and the sync future (answering §3.4-1)

Files-as-source-of-truth is not in tension with future sync — it is the
standard local architecture *for* sync, and the plan for Google/Outlook was
sketched with it in mind:

- **How the providers sync.** Google Contacts speaks CardDAV
  (`carddav.google.com`, OAuth — infrastructure we already have from 1d.19);
  Outlook/O365 has no CardDAV and needs Microsoft Graph REST (the same
  io-msgraph avenue the phase 3 mail roadmap names). Both protocols are
  per-resource: each contact is one addressable object with a server
  revision (ETag / Graph `@odata.etag`).
- **Why that favors the vdir.** One `.vcf` per UID maps 1:1 onto the
  protocol's unit of sync — sync is per-file compare-and-swap. This is
  exactly why the vdirsyncer/pimsync/khard ecosystem standardized the vdir
  layout as the local replica format for CardDAV.
- **What sync adds when it lands (phase 5 CardDAV / Graph): db
  *bookkeeping*, not db *storage*.** A `contact_sync` table (in `mail.db` or
  a sibling): remote-id ↔ UID map, last-seen server ETag per contact, the
  collection sync token, and **tombstones** — the one thing files cannot
  express, since a deleted file looks identical to one that never existed.
  [persistence.md](../documentation/persistence.md) §3 already designs this
  split. Losing that db costs a full re-compare against the server; it can
  never lose a contact.
- **Why not db-primary.** The persistence design's core promise is that the
  cache tier is deletable (`rm -rf ~/.cache/nitidus` loses nothing);
  contacts in the db would either break that or demand a second,
  backup-critical database. Files are human-recoverable (a corrupted store
  is N−1 good files, not one bad blob), diffable, git-versionable, and at
  address-book scale (hundreds to low thousands, fully in memory) a query
  engine buys nothing. Evolution, DAVx5, and every vdirsyncer client sync
  fine with exactly this shape.

Conclusion: no change to 1e.22, and nothing extra to prepare now — the
sync-era additions are purely additive db tables beside the files.

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **Domain: many cards in, many cards out, photo write.**
   `nitidus-contacts` gains `parse_all` (iterate calcard's parser to
   `Eof`; malformed entries become per-card issues, good cards proceed,
   UID injection as at load), `store::write_export` (all cards, vCard
   4.0, atomic tmp+rename, errors if the target exists), and
   `Contact::set_photo_jpeg` (replace-or-add the PHOTO entry from JPEG
   bytes via the validated line path). Unit tests incl. a
   Google-export-shaped multi-card fixture and a mid-file broken card.
2. **`:import-contacts` / `:export-contacts`.** New
   `contacts/transfer.rs`: import (leading-`~` expansion, `parse_all`,
   skip-existing-UID, atomic save + upsert per card, statusline
   `imported N, skipped M, failed K` with details logged) and export
   (refuse existing target, atomic write). Command args optional: a
   path runs directly; no-arg export prompts prefilled with
   `~/nitidus-contacts.vcf`; no-arg import prompts for a path until the
   explorer lands next phase. End-to-end tests over temp files.
3. **File explorer modal.** `ratatui-explorer` dependency; an
   `ExplorerState` modal (open/route/close like prompt and overlay:
   keys forward to `FileExplorer::handle`, Esc cancels, Enter on a
   matching file fires an `on_pick` callback), themed from the theme
   resource, centered-panel rendering. `:import-contacts` with no
   argument opens it filtered to `.vcf`. Tests drive it with key
   events over a temp directory.
4. **`P`/`:set-photo`.** The explorer filtered to image extensions;
   the pick decodes via `image`, downscales to max edge 256, encodes
   JPEG, embeds through `set_photo_jpeg`, saves atomically. The
   detail-pane photo cache keys on content, not just UID, so the new
   photo shows immediately. Tests: photo set end to end on a temp
   vdir, downscale bounds, replace-existing-photo.
5. **Verification & smoke handoff.** Clippy + full workspace run with
   counts; seed a multi-card import fixture (one card overlapping an
   existing UID, one broken) and hand Norman the smoke checklist
   (import via explorer, export, set a photo, re-import the export).
   Fill §5–§7.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **366 passed, 0
  failed** (was 355 at branch start).
- New coverage:
  - domain: `parse_all` over a multi-card input with a broken card
    between good ones (all good cards read, garbage reported, missing
    UID injected), pure-garbage input, `write_export` round-trip
    through `parse_all` and refuse-to-overwrite, `set_photo_jpeg` add
    then replace without stacking PHOTO entries;
  - app e2e: import skips existing UIDs with the exact statusline
    summary, export writes the whole book once and refuses a second
    time, no-arg export prompts prefilled, explorer pick drives an
    import end to end (extension filter proven by row count), Esc
    cancels cleanly, `:set-photo <path>` embeds an inline JPEG capped
    at 256px preserving aspect and re-setting replaces, no-arg `P`
    opens the browser.
- Live smoke (Norman): **PASSED** — over seeded fixtures
  (`~/nitidus-import-sample.vcf`: two new cards, one UID collision, one
  garbage line; `~/nitidus-photo-sample.png`: 512px portrait). Norman
  cleared the vdir before starting, so the collision card imported as
  new instead of skipping — consistent with the empty book, and the
  skip path stays covered by the e2e test. Browser-driven import,
  set-photo with immediate redraw, export, and refuse-to-overwrite all
  behaved as expected.
- Post-smoke headless check: the smoke's export re-parsed through
  `parse_all` with zero issues — 3 cards, exactly one inline photo
  (Katherine Johnson, valid JPEG magic bytes).

## 6. Implementation Report

- `parse_all` iterates calcard's parser to `Eof`; the existing
  single-card `from_vcf` now shares the same UID-injecting
  constructor. Export reuses the per-card writer — the export file is
  simply every card's `to_vcf` concatenated, which `parse_all` proves
  round-trips.
- Import is intentionally conservative: skip-existing-UID means
  re-importing the same Takeout file is idempotent and can never
  clobber local edits; the counts surface in the statusline and the
  details go to the log.
- The explorer became app infrastructure (`src/explorer.rs`), not a
  contacts-private widget: ratatui-explorer drives navigation and
  filtering, but rendering is ours (snapshot rows into the plurimus
  widget), so the panel matches the app's overlay chrome and the
  crate's own theme machinery goes unused. Enter picks files and
  descends directories; the extension filter always keeps directories
  visible. `RouterPlugin` owns the `ExplorerState` init so every
  embedder (tests included) gets the routing branch for free.
- `:set-photo` embeds rather than references: decode → `thumbnail`
  (aspect-preserving, long edge 256) → RGB flatten (JPEG has no
  alpha) → inline base64 through the same validated line path as
  every other mutation. The detail-pane photo cache now keys on a
  content fingerprint alongside the UID so a replaced photo redraws
  immediately.
- Follow-ups: path completion for the argument forms (with the
  command-line completion work), a remembered last-browsed directory
  for the explorer, per-contact selective export if ever wanted, and
  the explorer as a future picker for attachment saves.

## 7. Testing and Cleanup

- Cleanup scope: the branch diff vs main. Comments state invariants
  (skip-is-idempotency, the explorer's Enter pick-vs-descend split,
  the 256px embedding bound, fingerprint-keyed photo cache); no dead
  code — clippy silent, every helper has callers. One integration
  wrinkle found mid-build: the router's new explorer branch initially
  read a resource its embedders didn't have — `RouterPlugin` now owns
  the `ExplorerState` init, so tests and the app get it identically.
- Smoke artifacts removed (`~/nitidus-import-sample.vcf`,
  `~/nitidus-photo-sample.png`, and the smoke's
  `~/nitidus-contacts.vcf` export); the three imported contacts remain
  in the live vdir.
- Final verification after the smoke:
  `cargo clippy --workspace --all-targets` zero warnings;
  `CARGO_INCREMENTAL=0 cargo test --workspace` **366 passed, 0
  failed** (suite counts confirmed present).
