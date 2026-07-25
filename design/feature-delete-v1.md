# feature - Delete - v1

Pulled forward from 1f.25: single-message deletion, done with move-to-trash
semantics, plus the recorded 1c.17 follow-up fix — the IMAP delete currently
expunges the _whole folder_, not just the target message. Batch/visual-mode
deletion stays in 1f.25; this gives triage its most-missed verb now.

## 1. Current Design

- `MailBackend::delete_message` exists (1c.17, built for drafts): maildir
  removes the message file; IMAP adds `\Deleted` to the UID then runs
  **`EXPUNGE` on the entire folder** — it purges every `\Deleted`-flagged
  message the user may have had pending, not just ours. Recorded follow-up. Its
  only callers are internal: re-postponing a recalled draft and removing the
  source draft after a send.
- There is no user-facing delete: no key, no command, and `folders.trash` (which
  the wizard now writes for every account) is never read.
- No move primitive exists at all — `MailCommand` has flags, fetch, append,
  delete, folder ops. Recovering anything or filing anything means another mail
  client.
- Available io-imap machinery (unused so far): `ImapMessageMove` — UID MOVE (RFC
  6851; Gmail and Office 365 both advertise `MOVE`) returning the COPYUID triple
  — and `ImapRaw` for commands without a dedicated coroutine, which covers
  `UID EXPUNGE` (RFC 4315 UIDPLUS).
- Index ops today are optimistic: flag writes update `MailStore` immediately and
  the next sync confirms. `z` undo exists only for the send window; there is no
  delayed-operation machinery for index actions.
- Maildir folders are directories with `cur/new/tmp`; a move is a file rename
  across folder directories.

## 2. Proposal

1. **`MailBackend::move_message(folder, id, target)`** — the new primitive.
   IMAP: `UID MOVE` (error on servers that do not advertise `MOVE`; the
   COPY+DELETE fallback can come if such a server ever appears). Maildir: rename
   the message file into `target/cur`, preserving the flag suffix. Engine
   command + `JobDone`-style event wiring like the other folder-affecting ops.
2. **Fix the over-broad expunge**: `delete_message` on IMAP flags `\Deleted`
   then runs `UID EXPUNGE <uid>` via `ImapRaw` when the server advertises
   `UIDPLUS`, falling back to the current whole-folder `EXPUNGE` (with a warning
   log) otherwise. Gmail, O365, and Dovecot all advertise UIDPLUS, so the
   fallback is a corner case.
3. **`d` / `:delete`** on the index selection and in the pager:
   - in any folder except the account's trash → **move to `folders.trash`**,
     optimistically removed from the store (selection advances; pager closes to
     the index). The trash itself is the undo.
   - in the trash folder → permanent delete, gated by a `y/n` confirm prompt
     ("Delete permanently? (y/n)"), using the fixed `delete_message`.
4. **`:move <folder>`** — the same primitive, exposed generally: moves the
   selection to the named folder (completion offers the account's folder list).
   This is what makes delete recoverable from inside nitidus (open Trash,
   `:move INBOX`) and gives filing for free.
5. **Store/UI behavior**: optimistic removal from `MailStore` with the next scan
   as the reconciler, matching the flag-op philosophy; the destination folder's
   counts refresh via the folder list the backends already push after mutations.

Out of scope: batch/visual-mode delete and the `z` undo for destructive index
actions (1f.25 — the delayed-op machinery belongs with marking), Gmail label
semantics beyond what `[Gmail]/Trash` already does server-side, and
auto-emptying trash.

## 3. Discussion

### 3.1 R1 Questions

1. **Instant delete vs undo window.** Proposal: `d` moves to trash immediately
   (trash is the undo, `:move INBOX` recovers) — no 10s countdown like send. The
   outbox-style delayed executor would be real machinery better built once for
   1f.25 batch undo. Agree, or do you want the countdown now?
2. **Permanent-delete confirm.** `y/n` prompt only when deleting inside the
   trash folder — everywhere else `d` is silent (it is recoverable). OK?
3. **Pager delete.** `d` in the pager deletes the open message and returns to
   the index. Include, or index-only for v1?
4. **`:move` inclusion.** Confirm bundling the general `:move <folder>` command
   (it is the recovery path and ~free once the primitive exists). Argument
   completion from the folder list.
5. **Key choice.** `d` in index and pager contexts. Gmail-style `#` or
   mutt-style `D` variants can wait for keymap schemes (phase 2). Confirm.
6. **Smoke plan.** Live Gmail: send self a test mail, `d` it (verify it lands in
   `[Gmail]/Trash` server-side), `:move INBOX` it back, `d` again, then
   permanent-delete it from Trash (verify gone). OK to run headlessly with a
   pinned target per the smoke rules?

### 3.2 R1 Answers

1. agree
2. agree
3. include
4. yes
5. confirm
6. ok

## 4. Plan

Each phase leaves the workspace compiling with clippy clean and tests green.

1. **`move_message` primitive.** Trait method + `MailCommand::MoveMessage`
   + actor wiring (post-mutation folder-list refresh matching
   append/delete). Maildir: rename into `target/cur` preserving the flag
   suffix. IMAP: `UID MOVE` via `ImapMessageMove`. Scripted tests for
   both backends.
2. **Targeted expunge.** IMAP `delete_message` runs `UID EXPUNGE <uid>`
   (`ImapRaw`) when UIDPLUS is advertised; whole-folder `EXPUNGE`
   fallback logs a warning. Scripted tests for both paths, including
   proof that a bystander `\Deleted` message survives.
3. **App verbs.** `d`/`:delete` (index + pager contexts) and
   `:move <folder>`: trash detection against the account's
   `folders.trash`, `y/n` confirm inside trash, optimistic
   `MailStore::remove_envelope` with selection advance, pager closes to
   the index after deleting the open message.
4. **App tests.** Maildir-backed app: `d` lands the file in the trash
   folder and prunes the store; `d` inside trash prompts then deletes;
   decline keeps it; `:move` files to a named folder; pager `d` closes
   and deletes.
5. **Verification & live smoke.** Clippy + full run with counts. Gmail
   smoke per §3.1-6 with a pinned self-sent target: delete →
   `[Gmail]/Trash` server-side, `:move INBOX` back, delete again,
   permanent-delete from Trash → gone (bystander-safety observed via a
   second `\Deleted`-flagged control message left untouched). Fill
   §5–§7.

## 5. Verification

- `cargo clippy --workspace --all-targets`: zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace`: **319 passed, 0 failed**
  (was 312 at branch start).
- New coverage: maildir move (flag suffix survives; missing target
  errors), IMAP `UID MOVE` against the scripted server, both expunge
  paths (UIDPLUS advertised → the scripted server *forbids* a bare
  EXPUNGE; not advertised → old behavior), and four end-to-end app
  tests over maildir: `d` to trash with optimistic store removal,
  confirm/decline inside trash, `:move` to a named folder with
  unknown-folder refusal, pager `d` closing back to the index.
- Live Gmail smoke (headless, pinned targets per the smoke rules):
  - seeded a pinned target in INBOX and a `\Deleted`-flagged bystander
    in `[Gmail]/Trash` via imaplib;
  - `d` → "moved to [Gmail]/Trash", target server-verified in Trash and
    gone from INBOX;
  - sidebar into Trash, `:move INBOX` → "moved to INBOX",
    server-verified back;
  - `d` again, then in Trash `d` → "Delete permanently? (y/n)" → `y` →
    "deleted permanently"; target absent from INBOX, Trash, **and All
    Mail** (fully purged);
  - **the bystander survived with its `\Deleted` flag intact** — the
    targeted `UID EXPUNGE` proven live; the old code would have purged
    it.

## 6. Implementation Report

- `move_message` slotted into the existing backend shapes: maildir is a
  cross-directory rename preserving the info suffix; IMAP is
  `ImapMessageMove` with `uid: true`, COPYUID discarded (consumers
  re-sync rather than track the new id).
- The expunge fix threads a `advertises_uidplus()` capability check
  through the session; the `UID EXPUNGE <uid>` goes over io-imap's
  `ImapRaw` since no dedicated coroutine exists. The no-UIDPLUS
  fallback keeps the old whole-folder behavior and logs a warning.
- The verbs live in `index/remove.rs`; the pager shares them (its open
  message wins over the index selection as the target, and deleting it
  closes the pager). Optimistic removal reuses the flag-write
  philosophy — store row out immediately, next scan reconciles,
  `JobFailed` surfaces any server refusal.
- **Latent bug fixed en route**: the index window builder and selection
  anchor indexed `envelopes` through row entries that can be one frame
  staler than the store — impossible to hit until something removed
  rows mid-session. First row removal panicked the app in tests; both
  sites now resolve leniently and clamp the window.
- `:move` validates the destination against the account's folder list
  before touching anything, so a typo cannot optimistically vanish a
  message.
- Follow-ups: batch/marked delete and `z` undo stay with 1f.25;
  `:move` argument completion (the command line completes command
  names only today); Gmail's Trash auto-purge (30 days) makes "trash
  as undo" time-limited there, which is fine.

## 7. Testing and Cleanup

- Cleanup scope: the branch diff vs main. Comments reviewed — the new
  module docs state semantics (trash-as-undo, optimistic reconcile,
  bystander hazard on the fallback path) rather than narration; no
  dead code, clippy silent. The smoke's bystander message was expunged
  from the live Trash after verification.
- Final verification after the smoke:
  `cargo clippy --workspace --all-targets` zero warnings;
  `CARGO_INCREMENTAL=0 cargo test --workspace` **319 passed, 0
  failed** (suite counts confirmed present).
