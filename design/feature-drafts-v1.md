# feature - Drafts - v1

Roadmap item 1c.17, the last of phase 1c. Postpone and recall (server-synced
through the account's Drafts folder), compose crash recovery for the body files
that already survive, attachment add/remove on the review screen, and the
forgotten-attachment and empty-subject warnings before send.

## 1. Current Design

- `P` on review is a stub notice. `ComposeSession` carries headers, reply
  context, and a crash-surviving body file under `state_dir/compose/` — but the
  headers live only in memory, so a crash loses everything except the body text,
  and nothing ever recalls the orphaned files.
- `MailBackend::append_message` (1c.16) can write a draft into any folder with
  flags; there is **no message delete** — recalling and re-postponing would
  accumulate stale server drafts without one. io-imap has no UID-EXPUNGE
  coroutine (rfc4315 is APPENDUID only), but `\Deleted` + `ImapMailboxExpunge`
  covers a drafts folder; maildir delete is a file removal.
- `compose/build.rs` deliberately omits the Bcc header (envelope only) — correct
  for transmission, wrong for a draft, which must round-trip Bcc. mail-builder
  has `.bcc(...)` and `.attachment (content_type, name, bytes)` ready; the
  session has no attachment list and the review screen shows none.
- The prompt line and picker overlay give attachment UX for free (path prompt to
  add, picker to remove). `folders.drafts` defaults to `"Drafts"`; Gmail's is
  `[Gmail]/Drafts` (live config update).
- The outbox meta pattern (sidecar toml per staged thing) is proven crash-safe
  plumbing to copy for session persistence.

## 2. Proposal

### 2.1 Session persistence (crash recovery)

Every session writes a sidecar `<body-stem>.toml` (headers, reply context,
attachments) on creation and after every mutation (prompt submit, editor return,
attachment change); discard/postpone/send remove it with the body. Startup scans
`state_dir/compose/` for orphaned pairs: a notice reports the count and
`:recover` restores the newest into a full session at review (older orphans
remain until recovered or discarded in later passes).

### 2.2 Postpone and recall

- **Postpone (`P`)**: build the message in _draft form_ (Bcc header kept,
  attachments included), `append_message` to `folders.drafts` with
  `\Draft \Seen`, delete the local session (body + sidecar), and if this session
  was itself recalled from a server draft, delete that original
  (`delete_message`: maildir file removal; IMAP `\Deleted` + EXPUNGE on the
  drafts folder).
- **Recall (`e` in the index, `:recall`)**: only meaningful when the viewed
  folder is the account's drafts folder (elsewhere a notice). Fetches the
  selected draft (the reply-intent machinery generalizes to a recall intent),
  parses headers — including Bcc — and attachments back into a `ComposeSession`
  at review, remembering the server draft for replacement on the next
  postpone/send.
- Sending a recalled draft also deletes the server original after `SendDone`
  (outbox meta carries the draft source).

### 2.3 Attachments

- `ComposeSession.attachments: Vec<PathBuf>`; the review screen lists them
  (name + size) under the headers; `a` prompts for a path (`~` expansion;
  missing file → notice, session unchanged), `x` opens the picker to remove one.
- `build` gains a mode: transmission (Bcc envelope-only) vs draft (Bcc header
  kept); both attach files via mail-builder with a minimal extension → MIME map
  (`application/octet-stream` fallback). Recalled drafts materialize their
  attachment parts into `state_dir/compose/<stem>-att/` files so the session
  stays file-based.

### 2.4 Send-time warnings

`y` runs checks before queueing, each a y/n prompt: empty subject
(`Send without a subject? (y/n)`), then forgotten attachment — body mentions
attach-words (`attach`, `attached`, `attachment`, case-insensitive, quoted lines
excluded) while the list is empty (`No attachment — send anyway? (y/n)`).
Declining returns to review.

### 2.5 Wiring

- Trait: `MailBackend::delete_message(folder, id)` (maildir, IMAP, mock) +
  `MailCommand::DeleteMessage`.
- Commands with summaries: `:postpone` becomes real, `:recall`, `:recover`,
  `:attach <path>`, `:detach`; review keys `a`/`x`; index key `e` (drafts folder
  only).
- Tests: sidecar round-trip + recovery, postpone → maildir draft with Bcc +
  attachment intact, recall reconstructing the session (headers, Bcc,
  attachments), re-postpone replacing the old draft, send-after-recall deleting
  the draft, both warnings (accept + decline), attachment add/remove
  integration. Live smoke: postpone on kenianbei → draft visible in Gmail
  (`[Gmail]/Drafts`, config updated) → recall → send.

## 3. Discussion

### 3.1 R1 Questions

1. **Delete semantics for drafts.** `delete_message` joins the trait; the IMAP
   impl marks `\Deleted` and expunges the drafts folder (whole-folder expunge —
   fine for drafts, and general message deletion stays a 1f.25 concern). OK?
2. **Recall key.** `e` on a selected message in the drafts folder (notice
   elsewhere), plus `:recall`. OK, or prefer Enter-in-drafts to recall instead
   of viewing?
3. **Crash recovery UX.** Startup notice with orphan count + `:recover`
   restoring the newest; no picker over orphans yet. Acceptable for v1?
4. **Attachment keys.** `a` add (path prompt), `x` remove (picker) on review.
   OK?
5. **Warning heuristics.** Attach-word list on unquoted body lines;
   empty-subject check always. Both y/n prompts, decline returns to review. OK?
6. **Live config + smoke.** `folders.drafts = "[Gmail]/Drafts"` for kenianbei;
   smoke does postpone → verify in Gmail → recall → send (to kenianbei itself,
   target pinned per the smoke rule). OK?

### 3.2 R1 Answers

1. ok
2. ok
3. ok
4. ok
5. ok
6. ok

## 4. Plan

Each phase leaves the workspace compiling, clippy-clean, and tests green.

**Phase 1 — delete_message.** Trait + maildir (file removal via the
existing find), IMAP (`\Deleted` replace + `ImapMailboxExpunge`),
mock; `MailCommand::DeleteMessage`; maildir + scripted-IMAP tests.

**Phase 2 — session sidecar + recovery.** `compose/persist.rs`:
sidecar toml beside the body (headers, reply context, attachments,
draft source), written on create/mutation, removed with the session;
startup orphan scan (notice + count) and `:recover` restoring the
newest. Unit round-trip + integration recovery test.

**Phase 3 — attachments.** Session `attachments` list, review render
block, `a` path prompt (`~` expansion, existence check), `x` removal
picker, `:attach`/`:detach`; `build` gains `BuildMode::{Send, Draft}`
(draft keeps Bcc header) and mail-builder attachments with an
extension MIME map. Unit tests for MIME/build; integration add/remove.

**Phase 4 — postpone + recall.** `P`: draft build → `append_message`
to `folders.drafts` (`\Draft \Seen`) → replace old server draft →
local cleanup. Recall intent (generalizing the reply intent into one
fetch-intent enum): `e`/`:recall` in the drafts folder fetches and
reconstructs the session (headers incl. Bcc, attachment parts
materialized to disk); `draft_source` flows through the session and
outbox meta so send/postpone replace the original. Integration:
postpone → maildir draft (Bcc + attachment intact) → recall → session
equality → re-postpone replaces → send deletes.

**Phase 5 — send warnings.** `y` chain: empty-subject prompt, then
attach-words-without-attachment prompt (unquoted lines only); decline
returns to review. Unit heuristic tests + integration accept/decline.

**Phase 6 — wiring + live smoke.** Keys/commands/summaries; live
config `folders = { save_sent = false, drafts = "[Gmail]/Drafts" }`;
smoke: compose → attach a file → postpone → verify in Gmail Drafts →
recall → verify attachment survived → send to self → verify arrival
and draft deletion. Record §5/§6.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **270 passed, 0 failed**
  (was 262 pre-feature: +2 delete_message tests, +2 build-mode/MIME
  unit tests, +4 drafts integration tests).
- Coverage: maildir delete + IMAP `\Deleted`+EXPUNGE, draft mode
  keeping Bcc and attaching files (send mode stripping Bcc), postpone
  → recall round trip preserving To/Bcc/subject/attachments with the
  local files cleaned, re-postpone replacing the old draft, crash
  recovery restoring a full session from the sidecar, and both send
  warnings (decline keeps review, accept queues).
- **Live smoke (full lifecycle on Gmail):** composed with an
  attachment, `P` → draft landed in `[Gmail]/Drafts` with
  `\Draft \Seen` and the attachment part (verified server-side);
  navigated there via the sidebar, `e` recalled the complete session
  at review; `y` sent it — the message arrived in INBOX as multipart
  with the attachment intact and the server draft was **deleted**
  (Drafts count back to 0).

## 6. Implementation Report

Implemented per plan, with these findings:

- **Pre-existing key conflict found by the tests:** the compose
  context bound `b` twice — `:compose-bcc` from the composer item,
  silently overwritten by `:sidebar` from a later default. The Bcc
  prompt had been unreachable by key since then; the sidebar toggle
  is now Tab-only within compose.
- Postpone accepts drafts with an empty To (a placeholder recipient
  header keeps mail-builder honest; recall shows the field empty
  again). A present-but-unparseable To still errors.
- The recall path generalized the reply intent into
  `IntentPurpose::{Reply, Recall}` — one fetch-and-park mechanism for
  both.
- The review screen's attachment block was missed in the first pass
  and caught during the live smoke (tests asserted session state, not
  render) — added.
- **[Gmail]/All Mail is visible to the IMAP backend** (mbsync's
  exclusion list never applied here); an accidental folder switch
  full-scanned its 3.9k messages. Works, but a folder-exclusion
  config key is a worthwhile follow-up before 100k-message accounts.
- Splits for the 300-line rule: `compose/recall.rs` out of
  `drafts.rs`.
- Follow-ups: orphan picker for multi-crash recovery; attachment
  forwarding now unblocked for a small follow-up; folder exclusions.

## 7. Testing and Cleanup
