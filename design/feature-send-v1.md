# feature - Send Pipeline - v1

Roadmap item 1c.15. Turning the review screen's `y` into a real send:
mail-builder constructs the RFC 5322 message, a crash-safe outbox queue under
the state dir holds it through an undo-send delay, io-smtp (or a sendmail pipe)
transmits it on the engine runtime, and the statusline shows countdown → sending
→ sent. Reply machinery and the Sent-folder copy are 1c.16; drafts are 1c.17.

## 1. Current Design

- The composer stages everything a send needs: `ComposeSession` (account, from
  identity, to/cc/bcc/subject strings, body file); `y` currently prints a stub
  notice and keeps the session.
- Config is ready: `Outgoing::Smtp { host, port (587), encryption (starttls) }`
  and `Outgoing::Sendmail { command }` parse today but nothing consumes them;
  SMTP credentials can reuse the account's `Auth` through
  `secrets::resolve_password` (the IMAP path already does).
- The engine owns a tokio runtime and an event channel; the IMAP work built the
  reusable pieces a transmitter needs: a rustls TLS/plaintext stream type and
  the async coroutine-pump pattern (both currently private to
  `nitidus-mail/src/imap/`).
- **io-smtp 0.2** (verified from source): the same sans-IO coroutine
  architecture as io-imap — greeting/EHLO/STARTTLS/AUTH coroutines plus a
  composite `SmtpMessageSend` chaining MAIL FROM, one RCPT TO per recipient, and
  dot-stuffed DATA; SASL PLAIN/LOGIN/XOAUTH2 included; no Fragmentizer (resume
  takes bytes directly), so the pump is a sibling of the IMAP one, not shared
  code.
- **mail-builder 0.4** (spec's pick): builder-style RFC 5322/MIME construction —
  address headers, Date, Message-ID, text bodies.
- The statusline's left segment already composes tab · folder · engine summary ·
  position; a send status slots in the same way.

## 2. Proposal

### 2.1 Message construction

`compose/build.rs` (app side): `ComposeSession` → mail-builder → bytes.
From/To/Cc parsed from the comma-separated strings (`Name <addr>` and
bare-address forms); **Bcc recipients go to the envelope only, never the
headers**; Date and Message-ID generated; the body file's contents as the text
body. Empty To (no parseable recipient) fails `y` with a notice before anything
is queued.

### 2.2 Outbox queue (crash-safe)

`state_dir/outbox/<stamp>.eml` (the built message) + `<stamp>.toml` (account,
envelope from, recipient list, the compose body path, `send_at` epoch). `y`
writes the pair, drops the review screen back to the index, and starts the
countdown — the compose session's body file is _kept_ until transmission
succeeds, so undo can restore the full session. On success both outbox files and
the body file are removed.

### 2.3 Undo-send

Default delay 10 s. While an entry is pending the statusline shows
`sending in Ns · z undoes`; `z` (index context) deletes the outbox entry and
reopens the compose session exactly as it was (headers from the meta file, body
file untouched). At expiry the app hands the entry to the engine and the status
becomes `sending…`, then a transient `sent`. Failures keep the outbox entry,
surface the error, and `m` still resumes a restored session.

### 2.4 Transmission

`nitidus-mail` grows a `send` module and the engine an entry point:
`engine.submit(account, outgoing, envelope, bytes, job)` spawning a task on the
mail runtime; completion emits a new `MailEvent::SendDone { account, job }`,
failure the existing `JobFailed`. Two transports behind one enum:

- **SMTP (io-smtp)**: TCP → TLS (or greeting → EHLO → STARTTLS → TLS → EHLO
  again) → AUTH PLAIN when credentials are configured → `SmtpMessageSend` →
  QUIT. The rustls stream and pump glue move from `imap/` to crate-level
  `net.rs` so both protocols share them.
- **Sendmail pipe**: run the configured command, recipients appended as
  arguments, the (Bcc-free) message on stdin; non-zero exit is the error text.

### 2.5 Wiring

- `OutboxState` resource (pending entries + countdown), ticked by an app system;
  `OutboxStatus` feeding the statusline segment.
- Startup scans `state_dir/outbox`: entries whose delay elapsed are submitted
  immediately (the user already committed them), with a notice; parse failures
  surface and stay put.
- `z` → `:undo-send` (index context); command summaries throughout.
- SMTP credentials resolve at registration time alongside IMAP's (accounts with
  `outgoing.smtp` but unresolvable auth get a startup notice; sending then
  errors clearly).
- Tests: build.rs unit tests (headers, Bcc handling, recipient parsing), outbox
  round-trip unit tests, an integration test driving y → outbox file → undo →
  session restored, and a scripted SMTP server (the imap fake-server pattern,
  simpler dialogue) covering EHLO/AUTH/MAIL/RCPT/DATA and failure surfacing.
  Live smoke: send from kenianbei to norman.kerr.dev through smtp.gmail.com and
  watch the message arrive via the running sync.

## 3. Discussion

### 3.1 R1 Questions

1. **Undo model.** `y` queues with a 10 s countdown; `z` restores the full
   session (body file kept until transmission succeeds). Delay hard-coded this
   item, config key when the settings batch lands. Confirm?
2. **Startup recovery.** Leftover outbox entries whose time elapsed auto-send at
   startup with a notice (they were committed by `y`); failed entries persist
   and retry the same way. OK, or would you rather nothing sends without a fresh
   confirmation after a crash?
3. **Engine boundary.** Submission as an engine-runtime task with
   `MailEvent::SendDone`/`JobFailed`, io-smtp pumped like io-imap, and the TLS
   stream helpers promoted from `imap/` to a shared `net` module. Confirm?
4. **Sendmail semantics.** Recipients as command-line arguments plus the
   Bcc-free message on stdin (works with sendmail and msmtp without `-t`
   config). Confirm?
5. **Recipient parsing.** v1 accepts comma-separated `addr` / `Name <addr>`
   forms; full address-list validation and autocomplete stay with 1e.23.
   Empty/unparseable To fails `y` with a notice. OK?
6. **Live smoke.** The verification sends a real test email kenianbei →
   norman.kerr.dev via smtp.gmail.com (app password auth) and confirms arrival
   through the running sync. OK to send?

### 3.2 R1 Answers

1. confirm, we may want to add comfy-toast ratatui widget library for alerts,
   including a timer toast with undo option, if it can work well with our
   current ui library.
2. ok
3. ok
4. ok
5. ok
6. ok

### 3.3 R2 Notes

1. **ratatui-comfy-toaster works (R1-1).** Verified: 0.6.2 resolves
   against our ratatui 0.30 workspace cleanly. It is a toast *engine*
   (queue, timing via `tick()`, auto-dismiss durations, progress-bar
   styles, placement) rendered through a widget — which slots into
   plurimus as one more high-`WidgetOrder` entity ticked per frame.
   We use it **display-only**: the undo-send countdown becomes a timed
   toast with a progress bar and a `z undoes` caption, send success a
   short success toast, and send failure a sticky error toast. Its
   mouse/shortcut/action-channel features stay unused — the router
   keeps every key (same boundary as always); `z` reaches undo through
   the normal binding path. The statusline keeps non-send duties.

## 4. Plan

Each phase leaves the workspace compiling, clippy-clean, and tests green.

**Phase 1 — shared net module.** Move the rustls stream type and TLS
connect helpers from `imap/stream.rs` to `nitidus-mail/src/net.rs`
(imap re-exports; behavior unchanged, existing imap tests prove it).

**Phase 2 — transmission in nitidus-mail.** `send/` module:
`OutgoingTransport::{Smtp(SmtpConfig), Sendmail { command }}`,
`SendEnvelope { from, recipients }`; the SMTP flow (greeting → EHLO →
STARTTLS upgrade when configured → AUTH PLAIN when credentialed →
`SmtpMessageSend` → QUIT) pumped over the shared stream with an
io-smtp sibling of the imap pump; the sendmail pipe (recipients as
args, message on stdin). `MailEngine::submit(account, transport,
envelope, bytes, job)` spawns on the runtime and emits new
`MailEvent::SendDone { account, job }` or `JobFailed`. Scripted SMTP
fake-server tests (greeting/EHLO/AUTH/MAIL/RCPT/DATA happy path,
rejected RCPT, auth failure) plus a sendmail test against a script.

**Phase 3 — build + outbox in the app.** `compose/build.rs`:
recipient parsing (`addr`, `Name <addr>`, comma lists — unit tested),
mail-builder construction with Date/Message-ID, Bcc envelope-only;
`outbox.rs`: `<stamp>.eml` + `<stamp>.toml` pairs, `OutboxState`
countdown ticking, submission at expiry, `SendDone` cleanup (outbox
pair + compose body file), failure retention, startup scan with
notice, `z`/`:undo-send` restoring the full session. `y` rewires from
stub to build → queue → index. Integration tests: y writes the pair
and clears the session, z restores it byte-for-byte, expiry submits
(fake SMTP server end-to-end through the engine), failure keeps the
entry.

**Phase 4 — toasts.** `toast.rs` plugin wrapping ratatui-comfy-toaster:
engine resource, plurimus widget (high order, content-region
placement), per-frame `tick()`; countdown toast with progress bar for
the pending send, success/error toasts on SendDone/JobFailed.
Statusline unchanged except dropping the interim send text.

**Phase 5 — live smoke + docs.** Pty smoke: compose on kenianbei,
`y`, countdown toast visible, `z` restore once, `y` again letting it
send through smtp.gmail.com, confirm arrival in norman.kerr.dev's
maildir via the running sync. Record §5/§6.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **251 passed, 0 failed**
  (was 238 pre-feature: +1 smtp parse test, +5 scripted-SMTP/sendmail
  engine tests, +3 build unit tests, +4 outbox integration tests).
- Coverage: EHLO/AUTH/MAIL/RCPT/DATA happy path with captured message
  bytes, rejected recipient and auth failures surfacing, sendmail args
  + stdin, Bcc envelope-only construction, y → pair + countdown, z
  restoring the session byte-for-byte, expiry submitting end to end
  with full cleanup (including the compose body), failure parking the
  entry with files intact, and startup recovery.
- **Live smoke: a real email went through.** Composed on kenianbei,
  `y` showed the comfy-toaster countdown (`sending in 8s — z undoes`,
  bottom-right, ticking); the app was killed mid-countdown, the next
  launch's recovery scan submitted the entry through smtp.gmail.com
  (TLS + AUTH PLAIN), the outbox drained, and the message arrived in
  norman.kerr.dev's inbox — confirmed via mbsync with our generated
  Message-ID intact.

## 6. Implementation Report

Implemented per plan, with these notes:

- The crash-window behavior proved itself accidentally: the smoke's
  first run died mid-countdown and the second run's startup scan
  delivered the message — exactly the R1-2 contract.
- ratatui-comfy-toaster integrated as a custom plurimus `DrawFn`
  component (the engine is `!Sync`/`!Clone`, so it rides in a `Mutex`
  on the entity — the crate's own recommendation — and plurimus's
  trait-query registry accepts external draw layers directly). Ticking
  happens in the draw call; the countdown re-shows once a second under
  dedup.
- Failed sends park an hour out rather than retrying per-frame;
  startup retries them afresh. A future `:outbox` review screen is the
  natural place to surface parked entries (deferred).
- `outbox` split into `mod`/`delivery` under the 300-line rule.
- Follow-ups: Sent-folder copy rides 1c.16 as planned; per-account
  send-delay config with the settings batch; toast for `sent ✓`
  currently rides the statusline (`SendDone` handler) rather than a
  toast — trivially movable when toast polish lands.

## 7. Testing and Cleanup
