# feature - Reply Machinery - v1

Roadmap item 1c.16. Reply, reply-all, and forward from a read message into the
existing compose flow: correct addressing (aliases-aware), quoted body with
attribution, `In-Reply-To`/`References` so replies thread properly everywhere
(including our own index), and the Sent-folder copy after a successful send.
Drafts and attachments are 1c.17.

## 1. Current Design

- The composer + send pipeline are complete for fresh messages: `ComposeSession`
  (to/cc/bcc/subject strings, body file) → prompts → `$EDITOR` → review → outbox
  → SMTP/sendmail. Nothing carries reply context: no `In-Reply-To`/`References`
  fields exist on the session and `compose/build.rs` never writes them
  (mail-builder has `in_reply_to`/`references` builders ready).
- The pager's `OpenMessage` holds everything a reply needs: account, folder, id,
  **raw bytes**, and the parsed `MessageView`; the plain body text is one
  `default_part` away. The index selection has only the envelope
  (subject/from/message-id/references — no body).
- `AccountConfig` has `email`, `aliases`, and `Folders { sent: "Sent", … }`;
  nothing consumes `folders.sent` yet.
- Backends can read and flag but not **append**: `MailBackend` has no
  message-write method. io-imap ships `ImapMessageAppend` (mailbox, bytes,
  flags + internal date options); maildir needs the classic tmp-write → `cur/`
  rename delivery; Gmail **auto-saves SMTP-sent mail** into `[Gmail]/Sent Mail`,
  so an unconditional append would duplicate every sent message there.
- The outbox meta already persists everything about a queued send and survives
  crashes; `SendDone` is the natural append trigger. Free keys: `r`, `R`, `f` in
  both index and pager contexts.

## 2. Proposal

### 2.1 Reply context on the session

`ComposeSession` gains `in_reply_to: Option<String>` and
`references: Vec<String>`; `build.rs` emits both headers (References = original
References + its Message-ID, RFC 5322 style). The outbox meta carries them so
undo restores a reply as a reply.

### 2.2 Starting a reply/forward (pager)

`compose/reply.rs` builds a pre-filled session from the open message:

- **Reply (`r`)**: To = `Reply-To` else `From`; subject `Re: …` (existing
  `Re:`/`RE:` not doubled); quoted body.
- **Reply-all (`R`)**: To = reply target + original To; Cc = original Cc; every
  address matching the account's `email` or `aliases` is dropped (a reply-all to
  yourself keeps the original To as recipients, mutt-style).
- **Forward (`f`)**: empty To; subject `Fwd: …`; body is an inline
  `---------- Forwarded message ----------` block with the weeded headers and
  the full text body. Attachment forwarding waits for 1c.17's attachment
  machinery.
- The body file starts with the attribution line (`On <date>, <name> wrote:`)
  and the original text part quoted with `> ` (the pager's quote machinery then
  colors it on review), the signature after. Flow skips the To/Subject prompts
  when they are pre-filled (reply goes straight to the editor; forward prompts
  To first).
- The original message keeps its `\Answered` flag promise: on successful send of
  a reply, the source message's flags gain ANSWERED through the existing
  optimistic + backend flag path (the outbox meta records the source
  account/folder/id).

### 2.3 Reply from the index

`r`/`R`/`f` in the index fetch the selected message first: a
`PendingReplyIntent` resource remembers the requested kind; when the `Message`
event lands the intent runs the same session builder. (No pager screen flash —
the intent bypasses opening the pager.)

### 2.4 Sent-folder copy

- `MailBackend` gains
  `append_message(folder, bytes, flags) -> Result<(), MailError>`: maildir does
  tmp-write → rename into `cur/` with the `S` flag; IMAP runs
  `ImapMessageAppend` (with `\Seen`); mock stores in memory.
- New `MailCommand::AppendMessage { folder, bytes, flags }` handled by the actor
  (errors surface as `JobFailed`).
- On `SendDone`, the outbox entry's meta drives the copy: the message bytes are
  appended to the account's `folders.sent` folder — **unless the account opts
  out**. New config key `folders.save_sent: bool` (default `true`); the docs and
  the live config set it `false` for Gmail accounts (Gmail files SMTP-sent mail
  into `[Gmail]/Sent Mail` itself).
- The sent copy re-syncs into view through the normal folder-change paths
  (watcher for maildir; the folder simply re-scans on view for IMAP).

### 2.5 Wiring

- `Action::Reply(ReplyKind)` (`Reply`, `ReplyAll`, `Forward`) +
  `:reply`/`:reply-all`/`:forward` commands with summaries; `r`/`R`/`f` bound in
  pager and index contexts.
- Tests: pure reply-builder unit tests (addressing with aliases, Re:/Fwd:
  subject handling, quoting + attribution, references chains); maildir append
  unit test; scripted-IMAP append test; integration tests (reply from pager
  pre-fills and threads, reply-all drops self, forward prompts To, sent copy
  lands in the maildir Sent folder, `save_sent = false` skips it); live smoke:
  reply to a real message on kenianbei, verify the reply threads under the
  original in the recipient's mailbox and that ANSWERED shows in the index.

## 3. Discussion

### 3.1 R1 Questions

1. **Key choices.** `r` reply, `R` reply-all, `f` forward, in both pager and
   index. OK?
2. **Reply flow shape.** Reply/reply-all skip the To/Subject prompts (they are
   pre-filled) and drop straight into `$EDITOR` with the quoted body; forward
   prompts for To first. Header tweaks stay one key away on review. Confirm?
3. **Gmail duplication.** `folders.save_sent = false` opt-out (default `true`),
   set for your Gmail accounts in the live config. OK, or would you rather
   default `false` and opt _in_ for non-Gmail accounts?
4. **Answered flag.** Mark the source message `\Answered` only after the reply
   actually sends (not at compose time). Confirm?
5. **Reply-all self-filtering.** Matching against account `email` + `aliases`,
   case-insensitive. Good enough until 1e's address book?
6. **Forward scope.** Inline text only (no attachment carry-over) until 1c.17.
   OK?

### 3.2 R1 Answers

1. ok
2. ok
3. ok
4. ok
5. yes
6. ok

## 4. Plan

Each phase leaves the workspace compiling, clippy-clean, and tests green.

**Phase 1 — append in nitidus-mail.** `MailBackend::append_message
(folder, bytes, flags)`: maildir tmp-write → `cur/` rename with flag
suffix; IMAP `ImapMessageAppend`; mock in-memory.
`MailCommand::AppendMessage` + actor arm (`JobFailed` on error).
Maildir unit test, scripted-IMAP append test, engine round trip.

**Phase 2 — reply context through the pipeline.** `ComposeSession`
gains `in_reply_to`/`references` and an optional `reply_source
{account, folder, id}`; `build.rs` emits the threading headers; the
outbox meta persists all of it (undo restores a reply as a reply);
`Folders` config gains `save_sent: bool = true`. Unit tests for header
emission and meta round-trip.

**Phase 3 — the reply builder.** `compose/reply.rs`: pure functions
from raw message + account config → session fields (reply target via
Reply-To/From; reply-all merge minus self by email+aliases; Re:/Fwd:
prefix dedupe; attribution + `> ` quoting; forwarded-message block),
`ReplyKind`, and `start_reply(world, kind)` consuming the pager's
`OpenMessage` — replies go straight to the editor, forward prompts To
first. `ComposeSession::create` takes initial body content (fresh
compose passes empty). Thorough unit tests on the pure parts.

**Phase 4 — index intent.** `ReplyIntent` resource: `r`/`R`/`f` in the
index fetch the selection with a remembered kind; the engine drain
parks the arriving raw message on the intent instead of the pager; an
exclusive system consumes it into `start_reply`. `JobFailed` clears
the intent.

**Phase 5 — sent copy + answered flag.** `SendDone` handling: take the
completed outbox entry, and before file cleanup (a) if `save_sent`,
send `AppendMessage` of the built bytes to `folders.sent` with
`\Seen`, (b) if a reply, set the source message's flags |= ANSWERED
(optimistic store write + backend SetFlags). Live config gains
`save_sent = false` for the Gmail account.

**Phase 6 — wiring + verification.** `Action::Reply(ReplyKind)`,
commands with summaries, `r`/`R`/`f` in pager and index contexts.
Integration tests per §2.5. Pty smoke: reply to a real kenianbei
message, confirm `Re:` + quoting in the editor file, send it, verify
the reply threads under the original for the recipient and ANSWERED
appears; record §5/§6.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **262 passed, 0 failed**
  (was 251 pre-feature: +2 append tests, +4 reply-seed unit tests, +5
  reply integration tests).
- Coverage: maildir delivery + IMAP APPEND, reply targeting Reply-To
  with Re: dedupe and reference chains, reply-all dropping self and
  aliases, forward block, reply-from-pager pre-fill skipping prompts,
  reply-from-index via the fetch intent (no pager flash), forward's To
  prompt, sent copy landing in `.Sent` with `In-Reply-To` intact, the
  answered flag on the source, and `save_sent = false` skipping the
  copy.
- **Live smoke (real thread on Gmail):** self-sent a root message on
  kenianbei, replied to it with `r` from the index; the sent reply
  carries `In-Reply-To`/`References` of the root (verified server-side
  in Sent Mail), the root gained `\Answered` on the server, no
  duplicate Sent copy (Gmail self-files; `save_sent = false`), and the
  reply nests under the root with the `↳` glyph in our own threaded
  index once it arrives back through IDLE.

## 6. Implementation Report

Implemented per plan, with these findings:

- **The smoke caught a missed step and taught a lesson.** The first
  live run surfaced `IMAP APPEND failed: NO Folder doesn't exist` —
  the planned `save_sent = false` for the Gmail account had not been
  applied to the live config (now fixed). Worse, the reply itself was
  choreographed too early: the just-sent root had not yet arrived in
  INBOX, so the index selection was the newest existing message (a
  Voya Financial promo) and the test reply — quoted marketing text
  plus the fake-editor line — went to its reply address. The machinery
  behaved exactly right (correct threading headers to the message
  actually selected); the smoke script did not. Recorded in the pty
  memory: never reply-smoke against a live inbox without pinning the
  selection to a known message first.
- The answered-flag STORE also needs the app alive long enough for the
  actor queue to drain after `SendDone`; the corrected run verified it
  lands server-side.
- `ImapMessageAppend`'s error path (NO before the literal) leaves the
  session usable — subsequent commands proceed normally.
- Splits for the 300-line rule: `compose/intent.rs` (the index
  fetch-then-reply state) and `outbox` aftermath moved into
  `delivery.rs`.
- Follow-ups: attachment forwarding (1c.17); a `sent ✓` toast;
  flag-merge semantics on answered (current bases off the store
  snapshot; a stale store could drop `\Seen` set elsewhere — the
  1f/IMAP polish rounds should switch to `+FLAGS` add semantics).

## 7. Testing and Cleanup
