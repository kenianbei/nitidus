# Nitidus — Design Specification

Nitidus is a terminal email client written in Rust, using the himalaya v2
libraries (Pimalaya io-\*) for email handling, and bevy_ratatui/plurimus for the
TUI.

## Features

### Core Features

Everything needed to daily-drive a standard IMAP account with a better-than-mutt
experience.

#### Accounts & Sync

- Built-in account configuration wizard.
- Multiple first-class accounts (identity, aliases, folder mapping, signatures
  per account).
- IMAP backend (io-imap): IDLE push, header caching, incremental sync
  (CONDSTORE/QRESYNC), reconnect with visible connection status.
- Maildir backend (io-maildir), mbsync/offlineimap-compatible.
- OAuth2 (io-oauth): Google installed-app + Microsoft device-code flows, token
  refresh, OS keyring or credential-command storage.
- SMTP send (io-smtp) + sendmail pipe.

#### Folders & Index

- Folder sidebar: tree mode, unread counts, collapse, create/delete/rename.
- Virtualized threaded message list (100k+ messages without slowdown).
- Threading (server THREAD via io-imap, client fallback): collapse/expand,
  thread-scoped operations, jump-to-parent; sort decoupled from threading.
- Full sorting suite (date/from/subject/size/flags/threads, reverse, secondary
  sort).
- Basic limit/filter + incremental search with highlight and next/prev.
- Marking/batch operations: by hand, visual mode, whole thread.
- Flags (read/flagged/replied), trash-first delete, archive as a safe single-key
  verb.
- Undo (`z`) for archive/delete/move.
- Configurable index columns and theme-driven row styling.

#### Pager

- Header weeding/ordering, smart wrap, format=flowed, quote coloring +
  skip-quoted.
- Styled native rendering: quote levels, link list with open/copy, signature
  dimming.
- MIME part switcher; attachment list with save/open.
- HTML tier 1: native HTML→styled text, remote content blocked by default.
- Mark-read delay (peek).

#### Compose

- External `$EDITOR` composing (TUI suspends; sync continues).
- Review screen before send with keybinding cheat-sheet.
- Reply/reply-all/forward with proper quoting and threading headers.
- Attachments, forgotten-attachment warning, empty-subject warning.
- Postpone/recall drafts (server-synced); crash recovery of in-progress
  compositions.
- Outbox queue: async send with progress, failed sends never lost, undo-send
  delay.
- Contact autocomplete in To/Cc/Bcc.

#### Contacts

- Contact book tab: table view, 3-pane detail view, popup property editors
  (vCard 4.0; vcard_tui's design as reference only).
- Per-contact `.vcf` persistence (vdir layout, khard-interoperable),
  import/export.
- Address harvesting from mail traffic into frecency-ranked autocomplete.
- `:add-contact` from sender; `:compose-to` from contact.

#### Keys, Commands, Config

- Modal per-context keymaps, multi-key vim-style sequences, chord timeout hints.
- Everything-is-a-command: `:` line with history + completion; keys bind to
  command strings.
- Complete out-of-box defaults; `:help keys` live table.
- TOML config + keybinding files, all optional; no plaintext secrets.
- Seed-color theme system with derived interaction states.
- Tabs as the universal container (index, pager, compose, contacts).

### Roadmap

Post-MVP phases (2–5) are tracked in [roadmap.md](roadmap.md).

## Dependencies

Crates required by the Core Features, grouped by layer. Versions and maintenance
status are verified in [rust-libraries.md](rust-libraries.md); roadmap-phase
dependencies (chromiumoxide, gpg shell-out, libdav, tantivy, notmuch CLI) are
excluded until their phase.

- **bevy** (0.18, `MinimalPlugins` + `bevy_state`) — ECS backbone: app
  lifecycle, plugin-per-screen modularity, schedules, change detection driving
  the reconcile rendering pattern, `States` for input modes.
- **bevy_ratatui** (0.11) — bridges bevy and the terminal: terminal
  setup/teardown, crossterm event forwarding into the ECS, render schedule,
  kitty keyboard protocol.
- **plurimus** (0.1) — widget layer: renderable widgets as queryable ECS
  components (`Widget`, layout closures, draw ordering), focus/hover/press
  interaction markers, per-widget input bindings.
- **ratatui** (0.30) — the rendering library itself: buffers, layout, text
  spans, styles, and the stock widgets (tables, lists, paragraphs, blocks).
- **ratatui-image** (11) — contact photo thumbnails in the contact book:
  auto-negotiates kitty/iTerm2/sixel/halfblock graphics per terminal.
- **tui-prompts** — single-line text inputs: command line, header fields, search
  prompts, property editors (driven via plurimus key passthrough).
- **image** (0.25) — decoding contact photos (and later inline images) into
  pixel data for ratatui-image.
- **io-imap** (0.2) — IMAP protocol: connect/auth (XOAUTH2), folder listing,
  envelope fetch, flag updates, moves, IDLE push, incremental resync
  (CONDSTORE/QRESYNC), server-side SORT/THREAD with client fallback.
- **io-maildir** (+ **io-m2dir**) — local Maildir store: read/write messages and
  flags in `new/`/`cur/` with `:2,` flag semantics, compatible with
  mbsync/offlineimap layouts.
- **io-smtp** — SMTP submission with STARTTLS/TLS and XOAUTH2; sendmail pipe
  fallback is a plain process spawn.
- **io-oauth** — OAuth2 flows for the wizard: Google installed-app (auth-code +
  PKCE) and Microsoft device-code grants, token refresh.
- **mail-parser** (0.11) — MIME decoding: headers, encoded-words, charsets,
  nested multipart, attachment extraction, RFC 5256 base-subject (feeds
  threading). format=flowed decoding is hand-rolled on top.
- **mail-builder** (0.4) — MIME construction for compose: RFC 5322 headers,
  multipart bodies, attachments, correct transfer encodings.
- **tokio** (1.x) — the mail engine's runtime: all network and disk I/O runs
  here, on a dedicated thread pool separate from the bevy schedule.
- **tokio-util** — `CancellationToken` for cancelling superseded sync and fetch
  jobs.
- **flume** — bounded channels bridging the tokio engine and the bevy world:
  commands in, events out, backpressure for large folder syncs.
- **rustls** / **tokio-rustls** — TLS for IMAP and SMTP connections, no OpenSSL
  system dependency.
- **rusqlite** (0.40, `bundled`) — the envelope/sync cache: envelope metadata at
  100k+ scale, folder sync cursors (UIDVALIDITY/UIDNEXT/ MODSEQ), harvested
  addresses; WAL mode, `user_version` migrations.
- **rmp-serde** — compact msgpack serialization for cache-tier blobs inside
  SQLite rows.
- **blake3** — content-addressing keys for the cached-body store (dedupes
  identical messages across folders).
- **tempfile** — atomic write-via-rename for every precious file (contacts,
  drafts, outbox, UI state).
- **etcetera** — XDG base-directory resolution for the config/data/state/cache
  split.
- **calcard** (0.3) — vCard parsing/serialization for the contact book: lenient
  with the 3.0-isms real exporters emit, vCard 4.0 output, CardDAV-ready for
  Phase 5.
- **uuid** — UID generation for new contacts (`.vcf` filenames and vCard UID
  properties).
- **html2text** (0.17) — HTML tier 1: html5ever-based HTML→styled spans with
  `RichAnnotation` (links, emphasis) mapped onto ratatui styles; table layout
  and width-aware wrapping. The `css` feature stays off: ammonia strips
  `<style>` blocks and `style=` attributes before rendering, so it could only
  ever see nothing — CSS-driven color is revisited with the Phase 4 tiers.
- **ammonia** (4.1) — HTML sanitization before any rendering: strips
  scripts/trackers, blocks remote content by default via its attribute filter,
  allowlists `cid:`/`mailto:` schemes.
- **textwrap** — plain-text body wrapping and quote-aware reflow in the pager
  and composer.
- **serde** + **toml** (1.x) — typed parsing of `config.toml`, `keys.toml`, and
  state files (strict for config, lenient defaults for state).
- **crokey** — parsing human-written key notation (`<C-x>`, `gg`, `dd`) from
  keys.toml into crossterm key events for the keymap trie.
- **nucleo-matcher** — fuzzy matching for pickers and completion: command names,
  folder jump, contact autocomplete ranking.
- **jiff** (0.2) — dates and times: RFC 2822 header parsing (tolerant of
  obsolete zone forms), local-time display, relative date formatting in the
  index.
- **notify** (8) + **notify-debouncer-full** — filesystem watching of Maildir
  `new/`/`cur/` directories for external delivery detection.
- **keyring** (4.1) — OS-native secret storage (Secret Service / Keychain /
  Credential Manager) for OAuth refresh tokens and passwords.
- **tracing** + **tracing-subscriber** — structured logging to the state
  directory (the terminal is owned by the UI).
- **thiserror** / **anyhow** — typed errors in the library crates, context
  chains at the application edge.
