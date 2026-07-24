# feature - Pager - v1

Roadmap item 1b.10. The message pager: fetch + MIME decode, header weeding, wrap
with format=flowed, quote coloring and skip-quoted, a MIME part switcher,
attachment save/open, and a link list. First consumer of the `Message` event
(unrouted since 1b.5) and of a second screen in the content region.

## 1. Current Design

- `MailCommand::FetchMessage { folder, id, job }` and
  `MailEvent::Message { raw }` exist end-to-end (maildir reads the full file);
  the drain logs `Message` as "unrouted until the pager exists".
- One screen: the index widget owns the whole content region (the shell
  explicitly does not draw there, per bugfix-index-highlight-v1). There is no
  screen-switching concept; the router's Normal-mode context is the constant
  `CONTEXT_INDEX`. `"pager"` is already reserved in `KNOWN_CONTEXTS`, and
  layered routing means a pager-context binding can shadow a global one (`q`).
- `Action::Cursor(Motion)` motions are index-only today; `apply_action`
  dispatches straight to `index::` ops.
- Flag writes go through the optimistic `flag_selected` path (store-first,
  `SetFlags`, watcher re-sync confirms).
- mail-parser is already a dependency (windowed header parse); it handles full
  MIME decode (multipart traversal, charsets, quoted-printable/base64).
  `textwrap` is specified in rust-libraries.md but not yet a dependency; `jiff`
  is present.
- HTML tier 1 (ammonia + html2text) is the _next_ item, 1b.11 — this pager
  renders text parts only.

## 2. Proposal

### 2.1 `nitidus-mail::message` — pure MIME view

`MessageView::parse(raw: &[u8]) -> MessageView`, bevy- and render-free:

- `headers: Vec<(String, String)>` in original order (for the full view) —
  weeding/ordering is a UI concern.
- `parts: Vec<PartView>` — the flattened MIME leaves:
  `{ kind: Text | Html | Other, mime: String, filename: Option<String>, text: Option<String>, size: usize, is_attachment: bool }`.
  Inline text/plain and text/html become switchable body parts; everything else
  (and anything with a content-disposition of attachment) lists as an
  attachment. Decoded text comes straight from mail-parser.
- `default_part()` — first text/plain, else first HTML, else none.
- Raw bytes are kept alongside (`OpenMessage` owns them) so attachment save
  writes the decoded part contents without a refetch.

### 2.2 Screens

- New `Screen` resource: `Index | Pager`. The router's Normal-mode context
  derives from it (`index` / `pager`); everything else about routing is
  unchanged.
- The pager spawns its own widget over the content region at startup, like the
  index. Both widget states carry an `active` flag set by their refresh systems
  from `Screen`; the inactive one renders nothing, so exactly one paints the
  region per frame.
- `:view` (index `<Enter>`) fetches the selected message and switches to
  `Screen::Pager` immediately with a "loading…" state; the drain routes
  `Message` into `PagerState`. `:close` (pager `q`) returns to the index with
  selection intact. `JobFailed` on the fetch returns to the index with a
  statusline error.

### 2.3 `PagerState` and rendering

- `PagerState { open: Option<OpenMessage> }`;
  `OpenMessage { account, folder, id, raw, view, part_index, scroll, show_all_headers }`.
- **Headers**: weeded default `From, To, Cc, Date, Subject` (present ones only,
  that order, styled); `:headers` (`H`) toggles the full original-order set.
- **Body**: the selected part's text, re-flowed per format=flowed (quote-aware
  unwrap of soft breaks, DelSp honored) then wrapped to pane width via
  `textwrap`; quote depth (`>` count) colors cycle through theme accents;
  signature (after a `-- ` line) dims. Scrolling is over wrapped lines with the
  same width/height feedback pattern the index uses (state carries last area).
- **Attachments**: footer section listing name/mime/size, with a
  selected-attachment cursor for save/open.
- **Links**: `http(s)://` URLs extracted from the body; `:links` (`l`)
  opens the overlay picker (feature-overlay-v1) over them and selection
  opens the browser. (Tier-1 HTML anchors feed the same list next item.)
- **Part switcher**: `]` / `[` cycle body parts (`:next-part` / `:prev-part`);
  statusline center shows `text/plain 1/2` while multiple parts exist.

### 2.4 Commands and bindings

| command                         | pager binding      | notes                             |
| ------------------------------- | ------------------ | --------------------------------- |
| `:view`                         | index `<Enter>`    | fetch + open selected             |
| `:close`                        | `q`                | shadows global quit in pager      |
| `:next`/`:prev`                 | `j`/`k`, arrows    | line scroll (motions reused)      |
| `:next-page`/`:prev-page`       | `<Space>`, PgUp/Dn | page scroll                       |
| `:first`/`:last`                | `gg`/`G`           | top/bottom                        |
| `:next-message`/`:prev-message` | `J`/`K`            | adjacent index row, stay in pager |
| `:headers`                      | `H`                | toggle weeding                    |
| `:skip-quoted`                  | `S`                | jump past current quote block     |
| `:next-part`/`:prev-part`       | `]`/`[`            | MIME part switcher                |
| `:save-part`                    | `s`                | save attachment (picker if many)  |
| `:open-part`                    | `o`                | temp file + `xdg-open`, detached  |
| `:links`                        | `l`                | link picker → open in browser     |

`Action::Cursor` motions dispatch on `Screen`: index ops when the index is
active, scroll ops in the pager — one vocabulary, two surfaces. `J`/`K` reuse
the fetch path with the index's next/prev selection, so the index cursor follows
along underneath.

### 2.5 Side effects

- **Mark read on open**: opening a message sets SEEN through the existing
  optimistic flag path (so the maildir rename + re-sync just work).
  Peek/mark-read-delay is spec'd but deferred to a config item.
- **Save**: decoded part bytes written to `~/Downloads` (created if missing),
  filename from the part (sanitized, uniquified on collision); a config override
  can come later with the config growth.
- **Open**: same bytes to a temp file, `xdg-open` spawned detached (never blocks
  the frame loop).
- Body cache (`bodies/` content-addressed store from persistence.md) is deferred
  to the IMAP item — maildir fetch is a local file read.

### 2.6 Dependencies

`textwrap` (workspace + bin). Everything else is already present.

## 3. Discussion

### 3.1 R1 Questions

1. **Screen model** (§2.2): full-content pager replacing the index
   (neomutt-style), `Screen` resource driving both widget visibility and the
   router context, `J`/`K` for adjacent messages without leaving. Confirm over a
   split-pane (index above, pager below)?
2. **Links UX** (§2.3): numbered inline markers + footer list +
   `:open-link <n>`. Alternative is a picker overlay (needs popup infrastructure
   that doesn't exist yet). Footer list OK for v1?
3. **Mark-read** (§2.5): immediate on open for v1, peek-delay later as config.
   OK?
4. **No text part fallback**: when a message has only HTML (tier 1 is next
   item), show the HTML part's decoded text raw (tags visible) or a placeholder
   ("HTML-only message — rendering lands with tier 1") with headers/attachments
   still usable? I lean raw text — ugly but readable, and it exercises the part
   plumbing.
5. **Save destination** (§2.5): hardcode `~/Downloads` for v1 (config override
   later)? And `xdg-open` detached for `:open-part`?
6. **Bindings** (§2.4): anything you'd change — particularly pager `q` shadowing
   quit, `<Space>` as page-down, and `]`/`[` for parts?

### 3.1 R1 Answers

1. confirm
2. Let's add an overlay system (may also require wiring focusable, tabbing, etc,
   which plurimus may be able to provide). If this requires it's own feature,
   let's knock that out before continuing with this.
3. ok
4. raw text
5. yep
6. sounds good

## 4. Plan

Deltas from R1 + the overlay landing: links and multi-attachment
save/open use the picker (no inline numbering, no attachment cursor —
`s`/`o` act directly with one attachment, open a picker with several);
HTML-only messages show the part's decoded text raw (R1-4); save goes
through a `SaveDir` resource (default `~/Downloads`) so tests can
redirect it.

**Phase 1 — `nitidus-mail::message`** (pure, tests green):

1. `MessageView::parse(&[u8])`: ordered `headers`, flattened `parts`
   (`PartKind::{Text, Html, Other}`, mime, filename, decoded `text`,
   size, `is_attachment`, source part index), `default_part()`.
   `part_bytes(raw, index)` re-parses for save/open (rare path, no
   duplicated memory).
2. Tests: multipart/alternative picks text/plain, HTML-only falls back,
   attachments listed with names/sizes, headers keep order,
   `part_bytes` round-trips.

**Phase 2 — screen + fetch flow** (bin):

1. `Screen` resource (`Index | Pager`); router context derives from it;
   `apply_action` cursor dispatch grows the pager branch.
2. `pager/` module: `PagerState` (loading job or `OpenMessage`),
   `open_selected` (fetch + mark-read via the flag path + switch),
   `close`, `adjacent_message` (`J`/`K` = index cursor move + reopen),
   drain routes `Message`/fetch-`JobFailed` into it.
3. Commands/actions: `:view`, `:close`, `:next-message`,
   `:prev-message`, `:headers`, `:skip-quoted`, `:next-part`,
   `:prev-part`, `:save-part`, `:open-part`, `:links`; pager context
   defaults per §2.4; index `<Enter>` → `:view`.

**Phase 3 — body pipeline + rendering**:

1. `pager/body.rs` (pure): format=flowed reflow (soft-break merge
   within a quote depth, DelSp), quote-depth classification, wrap to
   width preserving quote prefixes (textwrap), signature detection,
   link extraction; skip-quoted target computation. Unit tests.
2. `pager/render.rs`: styled header block, body lines (quote colors
   cycling theme accents, dim signature), attachment footer, loading
   and raw-HTML notice lines; scroll windowing with width/height
   feedback like the index.
3. `SaveDir` resource; save (sanitized, collision-uniquified filenames)
   and `xdg-open` (detached spawn); link/attachment pickers via
   `overlay::open_picker`.

**Phase 4 — tests**: body pipeline units; integration (`tests/pager.rs`
over a real maildir fixture): `:view` opens + marks read + `Screen`
flips, `q` returns with selection intact, `J`/`K` walk, `H` toggles
headers, part switch on multipart, save writes into a temp `SaveDir`,
links picker opens with extracted URLs, fetch failure falls back with a
warning.

**Phase 5 — verification**: clippy, full workspace counts, isolated
mail build + no-bevy check, pty smoke (fixture with quoted reply +
link + attachment: open, scroll, `H`, `l` picker visible — the
overlay's first visual proof — `q` back), real-gmail spot check.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **170 passed, 0
  failed** (was 151): nitidus unit 95 + index 5 + overlay 6 + pager 6,
  nitidus-contacts 1, nitidus-mail 14+7+8+6 + message 6,
  nitidus-ui-kit 16.
- Pager integration tests (real maildir, no watcher): open marks read
  optimistically and flips `Screen`; close returns with selection
  intact; `J`/`K` walk adjacent messages; the part switcher reports
  `text/plain 1/2` → `text/html 2/2`; save writes the decoded
  attachment into a redirected `SaveDir` and uniquifies on collision;
  `:links` opens the picker; a failed fetch falls back to the index
  with a warning.
- pty smoke (90×24, fixture with quoted reply, link, signature, PDF
  attachment), pyte-replayed:
  - `<Enter>`: weeded headers, quote block, body, signature,
    `📎 q2-report.pdf application/pdf 8 bytes` footer.
  - `l`: the floating bordered links picker over the pager — the
    overlay's first visual proof — listing the extracted URL.
  - `H`: full original-order headers (X-Spam-Score, MIME-Version,
    Content-Type appear).
  - After open, the maildir file renamed to `:2,S` in `cur/` —
    mark-read reached disk.
- One real bug found by the pty run (invisible to the headless tests):
  opening a message marks SEEN → store change → index refresh → plurimus
  repainted the refreshed index widget *over* the pager. Fixed by
  gating the index render on `Screen` exactly like the pager — the
  standing rule is now: **inactive screens render nothing; draw order
  is never load-bearing across screens.**

## 6. Implementation Report

Implemented per plan with the overlay-era deltas. Notables:

- `nitidus-mail::message` stays byte-oriented and pure; `part_bytes`
  re-parses on save/open rather than keeping every part resident.
  `PartView` carries `is_flowed`/`delete_space` from the content type
  so the UI can reflow per RFC 3676.
- `pager/body.rs` is the pure pipeline (flowed reflow with
  space-unstuffing and DelSp, quote-depth classification tolerant of
  `>>`/`> >`, wrap preserving quote prefixes, signature detection, link
  extraction with punctuation trimming, skip-quoted targets) — all
  unit-tested without bevy.
- Scroll lives in the widget state only: cursor keys mutate it directly
  without touching `PagerState`, so scrolling never rebuilds the line
  list; rebuilds happen on open/part/headers changes (keyed) and reset
  scroll only when the key changes.
- Links are extracted from an unwrapped build (`usize::MAX` width) so
  wrapping can never split a URL out of recognition.
- `s`/`o` act directly with zero/one attachment and open the picker
  with several; both report success/failure on the statusline.
  `xdg-open` spawns fully detached.
- Fetch failure routes through the existing `JobFailed` drain arm:
  `PagerState::fail_fetch` + `Screen::Index` restore, statusline warn.
- The statusline center shows the part indicator only when idle (status
  messages and chord hints take precedence); shell inputs are grouped
  in a `SystemParam` struct now that they number seven.

Follow-ups:

- HTML tier 1 (next item) replaces the raw-HTML notice path and feeds
  anchors into the same link list.
- Mark-read peek delay as a config option (R1-3).
- `SaveDir` from config when the config surface grows.
- Line-list rebuild on `J`/`K` reopen is fine at mail scale; revisit
  only if profiling ever says otherwise.

## 7. Testing and Cleanup
