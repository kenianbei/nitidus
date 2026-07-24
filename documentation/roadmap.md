# Nitidus — Roadmap

All phases, in build order. Phase 1 is the Core Feature set from
[specification.md](specification.md), decomposed into an ordered build sequence:
load-bearing functionality first, optional core features later. Each numbered
item is intended to become one contrib design doc
(`design/{type}-{description}-v{n}.md` per the contributing workflow) as the app
is built from scratch. The later-phase split logic: Phase 2 is triage power that
is pure client-side work; Phase 3 is provider-specific work gated on
raw-protocol effort or new backends; Phase 4 carries heavy external
dependencies; Phase 5 integrates external ecosystems.

## Phase 1 — Core (MVP)

### 1a. Foundation (nothing works without these)

1. **Workspace scaffold** — cargo workspace (`nitidus-ui-kit`, `nitidus-mail`,
   `nitidus-contacts`, `nitidus` bin), pinned dependencies, AGENTS/rules wiring,
   logging to the state dir.
2. **App shell** — bevy app bootstrap (MinimalPlugins + ScheduleRunner +
   RatatuiPlugins + PlurimusPlugin), theme resource (ported from vcard_tui),
   root layout, statusline, tab-bar shell.
3. **Config loading** — XDG resolution, `config.toml` + `keys.toml` parsing
   (strict), compiled-in defaults, account definitions (no secrets).
4. **Action router** — Action enum + command-string parser, per-mode keymap trie
   (multi-key sequences, chord timeout hint), `:` command line with history +
   completion, `InputMode` states.
5. **Mail engine scaffold** — dedicated tokio runtime, `MailBackend` trait,
   per-account actor tasks, flume command/event channels, JobId + cancellation,
   `PreUpdate` event-drain system into resources.

### 1b. Read mail (first daily-drivable read path)

6. **Maildir backend** — first `MailBackend` impl (local, no auth): folder
   listing, envelope scan, flags, notify-based change watching.
7. **Envelope cache** — SQLite (WAL, migrations): envelope metadata, folder sync
   cursors, `MailStore` resource fed from the engine.
8. **Virtualized index** — windowed table over `MailStore` (100k rows),
   selection/scrolling, sorting suite, flag display + basic flag ops.
9. **Threading** — hand-rolled JWZ on mail-parser accessors, server THREAD
   passthrough hook, flat `ThreadRow` display list computed in the actor,
   collapse/expand, jump-to-parent, thread-scoped ops.
10. **Pager** — message fetch + MIME decode, header weeding/ordering, wrap +
    format=flowed, quote coloring + skip-quoted, part switcher, attachment
    save/open, link list.
11. **HTML tier 1** — ammonia sanitization (remote content stripped) + html2text
    styled spans in the pager.
12. **IMAP backend** — io-imap impl (password auth first): folder listing,
    incremental envelope sync (CONDSTORE/QRESYNC + batch streaming),
    flags/moves, IDLE push, connection status surfacing.
13. **Folder sidebar** — tree mode, unread counts, collapse,
    create/delete/rename, folder switching wired to sync cancellation.

### 1c. Send mail (closes the loop)

14. **Composer** — compose session state machine, `$EDITOR` suspend/resume,
    review screen with keybinding cheat-sheet, header prompts.
15. **Send pipeline** — mail-builder construction, outbox queue (crash-safe
    files), io-smtp submission + sendmail pipe, async progress in statusline,
    undo-send delay.
16. **Reply machinery** — reply/reply-all/forward, quoting + attribution,
    References/In-Reply-To, Sent-folder handling.
17. **Drafts** — postpone/recall (server-synced), compose crash recovery,
    attachment add/remove + forgotten-attachment and empty-subject warnings.

### 1d. Accounts & auth (multi-account, hosted providers)

18. **Secrets** — keyring integration + credential-command shell-out; 0600
    discipline.
19. **OAuth2** — io-oauth flows (Google installed-app, Microsoft device-code),
    token refresh, per-provider presets.
20. **Account wizard** — guided `:new-account` flow writing config + secrets;
    multiple accounts with per-account identity/folder mapping/signatures.

### 1e. Contacts (the differentiator)

21. **Contact book port** — vcard_tui plugins into `nitidus-contacts`
    (calcard-backed), contact tab with table + 3-pane detail + property
    editors + photos.
22. **Contact persistence** — vdir layout (one `.vcf` per contact), atomic
    writes, import/export.
23. **Autocomplete + harvesting** — `ContactIndex` prefix map, frecency-ranked
    harvested addresses from sync, To/Cc/Bcc popup completion, `:add-contact` /
    `:compose-to` bridges.

### 1f. Optional core (polish; MVP ships without regret if deferred)

24. **Search & limit** — incremental search with highlight + next/prev, stacking
    limit/filter, `:clear`.
25. **Batch operations** — visual-mode marking, mark-by-thread, batch
    flag/move/delete, undo (`z`) for destructive index actions.
26. **Index customization** — configurable columns, theme-driven row styling,
    conditional date display.
27. **Comfort features** — mark-read delay (peek), archive single-key verb
    tuning, auto-advance toggle, `:help keys` live table, mouse support pass.

## Phase 2 — Power triage & search

Pure client-side work.

- Full pattern/query language (neomutt-class operators) for
  limit/search/tag/color.
- Custom tags/labels; tag-driven operations.
- Saved searches as virtual folders (Outlook Search-Folders style).
- Snooze, mute, auto-advance after triage.
- Sweep-style bulk hygiene ("delete all from sender / keep latest / filter
  messages like these").
- Local full-text search (SQLite FTS) over cached mail.
- Unified inbox across accounts; selectable keymap schemes (mutt / Gmail
  flavored).
- Scheduled send; send-as aliases with per-alias signatures; Fcc routing;
  templates.
- One-key unsubscribe (List-Unsubscribe / RFC 8058); mailto: handling.
- Background periodic sync + new-mail notifications; config hot-reload.

## Phase 3 — Provider-native fidelity

Gated on raw-protocol effort or new backends.

- Gmail: label round-tripping (X-GM-LABELS), dedup (X-GM-MSGID), Gmail-fidelity
  threading (X-GM-THRID), server search passthrough (X-GM-RAW), archive-safe
  expunge handling — via raw commands or upstream io-imap contribution (or the
  io-gmail REST backend).
- Outlook: localized folder detection, correct Sent-Items APPEND, TNEF
  detection; Microsoft Graph backend (io-msgraph) for categories, Focused Inbox,
  server rules, search folders.
- Per-column pattern-driven index colors and conditional formatting; conditional
  date formats.

## Phase 4 — Rich content & crypto

Features with heavy external dependencies (Chromium, gpg, graphics protocols).

- HTML tier 2: inline images (`cid:` + attachments) via kitty/iTerm2/sixel with
  halfblock fallback.
- HTML tier 3: on-demand pixel-perfect rendering (headless Chromium → terminal
  graphics), cached, auto-degrading.
- Calendar invites: rendering + accept/tentative/decline iTIP replies.
- PGP via system gpg: sign/encrypt/verify/decrypt, per-account policies,
  per-recipient key rules, status in index/pager.

## Phase 5 — Ecosystem & automation

Integrations with external ecosystems.

- notmuch backend with tag workflows and saved queries.
- CardDAV contact sync (io-webdav / libdav).
- Hooks (folder-enter, message-received/sent, pre-send, startup/shutdown).
- Shell integration: `:pipe` (git am workflows), `:exec`.
- Quick-Steps-style named multi-action macros bound to keys.
- External query-command escape hatch for address lookup.

## Phase dependencies (deferred)

Added only when their phase begins; assessed in
[rust-libraries.md](rust-libraries.md):

- Phase 3 — hand-rolled Graph/Gmail REST clients (reqwest + serde) or the
  Pimalaya io-gmail / io-msgraph crates as they mature.
- Phase 4 — chromiumoxide (headless Chromium screenshots), gpg binary shell-out,
  calcard + hand-rolled iTIP replies.
- Phase 5 — libdav or io-webdav (CardDAV), notmuch CLI shell-out, tantivy (only
  if SQLite FTS5 proves insufficient).
