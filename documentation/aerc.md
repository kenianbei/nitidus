# aerc — Feature Analysis

Reference analysis of aerc ("a pretty good email client") for the nitidus
specification. aerc is a terminal email client written in Go, created by Drew
DeVault (2019), maintained since 2022 by Robin Jarry at `sr.ht/~rjarry/aerc`.

Design pillars: everything is asynchronous, everything is a command, external
programs do the specialized work (editor, pager, filters, address book,
crypto), and the whole UI is templated/themeable.

## 1. Mail Backends and Transports

### Incoming (`source =`)

- **IMAP**: `imap://` (STARTTLS), `imaps://` (TLS), `+insecure` variants, plus
  `+oauthbearer` / `+xoauth2` auth variants.
  - IDLE push support with tunables: `idle-timeout` (default 10s),
    `idle-debounce` (10ms).
  - Header caching to `$XDG_CACHE_HOME/aerc` (`cache-headers`,
    `cache-max-age`, default 30 days) — only fetches what the UI needs.
  - Connection robustness: `connection-timeout` (90s), TCP keepalive tunables.
  - `check-mail-include`/`check-mail-exclude` (regex) for periodic polling;
    `expunge-policy` for safe deletes.
  - Gmail X-GM-EXT-1 search extension (`:search -e filename:pdf is:starred`).
- **Maildir / Maildir++**: `maildir://path`, `maildirpp://path`; pairs with a
  syncer via `check-mail-cmd = mbsync -a`.
- **notmuch**: `notmuch://path`; `query-map` file maps names→queries as virtual
  folders in the sidebar; `exclude-tags`; `maildir-store` so real folder ops
  (move/archive/delete) work; `multi-file-strategy` for multi-file messages;
  `:tag`/`:modify-labels` mutates tags; `:query` creates ad-hoc virtual folders.
- **JMAP** (RFC 8620/8621): `jmap://`, `jmap+oauthbearer://`; outgoing via the
  same connection; LevelDB state/blob caching; `use-labels` turns mailboxes
  into Gmail-style labels with an `all-mail` virtual folder; `server-ping` for
  push.
- **mbox**: `mbox://` scheme; `aerc mbox:<file>` opens any mbox as a throwaway
  account; `:export-mbox` / `:import-mbox`.
- No POP3 (a noted gap).

### Outgoing (`outgoing =`)

- **SMTP**: `smtp://` (STARTTLS), `smtps://`; auth suffixes `+plain` (default),
  `+login`, `+none`, `+oauthbearer`, `+xoauth2`.
- **sendmail**: `outgoing = /usr/sbin/sendmail` (msmtp-compatible).
- **JMAP submission**.

### Credentials / OAuth2

Passwords inline or via `source-cred-cmd` / `outgoing-cred-cmd` (e.g.
`pass show ...`); OAuth2 params in the URL query (`token_endpoint`,
`client_id`, `client_secret`, `scope`); without a token endpoint the
"password" is used directly as the access token (works with external
refreshers like mutt_oauth2/oama).

## 2. Core UI Model

- **Tabs, tmux-style**: everything is a tab — one per account's message list,
  each opened message viewer, each composer, each `:term` shell, `:eml`
  previews. `:next-tab`/`:prev-tab`/`:change-tab` (with `-` history),
  `:move-tab`, `:pin-tab`; tab titles are Go templates.
- **Embedded terminal emulator** (vt100 widget): `$EDITOR`, the `less` pager,
  interactive filters (`w3m`), and full shells run *inside* a tab/pane.
  **Contrast with mutt**: mutt forks the editor and suspends its whole UI —
  while composing you cannot read mail and sync stalls; in aerc the editor is
  just another widget, so you can flip tabs mid-compose, watch new mail
  arrive, and reference other threads while writing. The single most-cited
  differentiator.
- **Message list**: template-driven columns (`index-columns` with per-column
  align/width), threading with configurable arrow glyphs, scroll offset,
  optional horizontal/vertical **split preview** (`:split`/`:vsplit`),
  auto-mark-read with delay, marking/batch selection ("pill" UI).
- **Sidebar (dirlist)**: folder list with unread/recent counts,
  template-driven, optional **tree mode** with collapse, configurable width
  (0 = hidden), per-account folder sort/filter.
- **Statusline**: template-driven left/center/right columns, pending
  ops/sync state (`{{.TrayInfo}}`), text or icon mode; ephemeral
  success/error messages styled by styleset.
- Extras: optional mouse support, popover completion UI, `:menu` popovers fed
  by shell commands (fzf pickers), dialog position/size config, quake-mode
  drop-down terminal, IPC so `aerc mailto:...` opens a composer in the
  running instance.

## 3. Command System

- `:` opens the **exline** (rebindable via `$ex`); command history; tab
  completion of commands, folders, addresses (fuzzy optional); commands accept
  flags like a CLI; `:prompt`, `:choose`, `:menu` build interactive flows;
  `:help <topic>` shows embedded man pages in a pager tab.
- **Keybindings are macros**: a binding simulates typing keystrokes, usually
  `:command<Enter>` — anything typeable is bindable, bindings can chain
  (`:mark -a<Enter>:archive flat<Enter>`) or inject raw keys into the
  embedded terminal (`:send-keys`).

### Commands by context

- **Global**: `help/man`, `new-account` (wizard), `cd`, `z`, `pwd`, `term`,
  `exec`, `echo`, `eml`, `send-keys`, `change-tab`, `next-tab`, `prev-tab`,
  `move-tab`, `pin-tab`, `unpin-tab`, `prompt`, `menu`, `choose`, `reload`
  (hot-reload conf/binds/styleset), `redraw`, `suspend`, `version`, `quit`.
- **Message list**: `cf` (change folder, cross-account `-a`), `check-mail`,
  `compose`, `filter`, `search`, `clear`, `next-result`/`prev-result`,
  `next`/`prev` (count or %), `select <n>`, `align`, `next-folder`/
  `prev-folder` (`-u` unread), `expand-folder`/`collapse-folder`, `mkdir`,
  `rmdir`, `sort` (arrival/cc/date/from/read/flagged/size/subject/to, `-r`),
  `toggle-threads`, `fold`/`unfold`, `toggle-thread-context`, `view`
  (`-p` peek), `split`/`vsplit`/`hsplit`, `query` (notmuch virtual folder),
  `export-mbox`/`import-mbox`, `recover` (crashed drafts), `bounce`/`resend`,
  `disconnect`/`connect`.
- **Message**: `archive` (flat/year/month), `copy`, `move` (cross-account,
  multi-folder strategies), `delete`, `read`/`unread` (`-t` toggle),
  `flag`/`unflag`, `modify-labels`/`tag`, `reply` (`-a` all, `-q` quote,
  `-c` close, `-T` template), `forward` (`-A` attach, `-F` full), `recall`
  (edit postponed draft), `envelope` popup, `pipe`, `patch …`,
  `accept`/`accept-tentative`/`decline` (calendar invites), `unsubscribe`
  (List-Unsubscribe), `mark`/`unmark`/`remark` (incl. `-T` whole thread,
  visual mode).
- **Message viewer**: `close`, `next-part`/`prev-part`, `open` (openers/xdg),
  `save`, `copy-link`, `open-link`, `toggle-headers`,
  `toggle-key-passthrough` (keys go to the filter pager, e.g. scroll w3m).
- **Compose**: `send` (`-t` copy-to override), `postpone`, `abort`, `edit`,
  `attach` (path, `-m` file-picker menu, `-r` command output), `detach`,
  `attach-key`, `cc`/`bcc`, `header` (arbitrary header edit), `multipart`
  (generate `text/html` alternative via converters), `next-field`/
  `prev-field`, `switch-account`, `sign`, `encrypt`.
- **Terminal**: `close` (plus all globals).

## 4. Keybinding Configuration (binds.conf)

- INI sections per context: globals at top, `[messages]`, `[view]`,
  `[view::passthrough]`, `[compose]`, `[compose::editor]`,
  `[compose::review]`, `[terminal]`; plus **scoped overrides**:
  `[messages:folder=Drafts]`, `[messages:folder~regex]`,
  `[context:account=Work]` (folder beats account).
- Syntax: `<key sequence> = <keystrokes to simulate>`; multi-key vim-style
  sequences (`rq`, `gi`); special keys `<Enter> <Tab> <C-x> <A-x> …`; empty
  RHS erases an inherited binding; `$noinherit = true` blocks inheritance;
  `$ex` and `$complete` rebindable.
- Trailing `# annotation` on a `[compose::review]` binding customizes its
  label on the review screen.
- **Defaults ship vim-flavored and complete**: j/k move, `J/K` folders,
  `gg/G`, `/` `?` `n/N` search, `v` visual-mark, `V` mark thread, Enter view,
  `d/D` delete, `a/A` archive, `rr/rq/Rr` replies, `f` forward, `c` compose,
  `t` term, `T` toggle threads, `zf/zF` fold, `Ctrl-t/n/p` tab ops — usable
  with zero config; `:help keys` shows the live table.

## 5. Filters (Rendering Pipeline)

- `[filters]` in aerc.conf maps a matcher → shell pipeline; the decoded part
  is piped through it, output shown in the pager (`less -Rc` default) with
  ANSI colors and OSC8 hyperlinks preserved.
- Matchers (most specific wins): exact MIME (`text/plain=colorize`), glob MIME
  (`text/*=bat`, `image/*=catimg`), header match (`from,name=…`), header
  regex (`subject,~Git(hub|lab)=…`), filename (`.filename,~.*\.csv=…`), and
  `.headers` post-processing of the header block.
- `!` prefix runs the filter **interactively with a TTY inside the embedded
  terminal** (e.g. `text/html=! w3m -T text/html` gives a navigable browser
  in the viewer); filters receive env: `AERC_MIME_TYPE`, `AERC_FORMAT`,
  `AERC_FILENAME`, `AERC_SUBJECT`, `AERC_FROM`, plus styleset-derived colors
  so filters can match the theme.
- **Built-in filters**: `colorize` (themes text/plain — quote levels, URLs,
  headers, signatures, **diffs/patches**), `wrap` (reflow respecting
  format=flowed), `hldiff`, `html` (w3m wrapper **sandboxed with
  dante/socksify to block network access**), `html-unsafe`, `calendar`
  (render text/calendar invites), `show-ics-details.py`.
- Calendar flow: filter renders the invite; `:accept` /
  `:accept-tentative` / `:decline` compose the proper iTIP reply.
- `[openers]` maps MIME types/URL schemes to programs for `:open` and
  `:open-link`; `[multipart-converters]` (e.g. `text/html=pandoc …`) powers
  `:multipart` for outgoing HTML alternatives.

## 6. Compose Flow

- `$EDITOR`/`$VISUAL` (or `[compose] editor=`) runs in the **embedded
  terminal**; header fields are UI widgets above it, or `edit-headers = true`
  puts headers in the editor buffer (mutt-style); `focus-body`,
  `format-flowed`, `lf-editor` options.
- **Review screen** after quitting the editor: shows
  recipients/subject/attachments and a cheat-sheet of the
  `[compose::review]` bindings (send, edit, attach, postpone, sign/encrypt,
  abort) — a praised safety net vs mutt's immediate-send prompt.
- **Address completion**: `address-book-cmd` runs any external command with
  `%s` = typed prefix, expecting mutt query-style TSV output — works with
  `khard email`, `abook`, `notmuch address`, and the bundled
  **`carddav-query`** helper. No native address book — deliberate.
- **Templates** (Go text/template): `new-message`, `quoted-reply`,
  `forward_as_body` replaceable; `:compose -T` / `:reply -T` per-invocation;
  template = headers + blank line + body, so templates can set headers.
- Safety/QoL: `no-attachment-warning` regex ("see attached" with nothing
  attached), `empty-subject-warning`, `reply-to-self` toggle, `strip-bcc`;
  `:postpone` + `:recall`; `:recover` finds buffers from crashed sessions;
  `:switch-account` mid-compose; per-account `signature-file`/
  `signature-cmd`; aliases with automatic From selection when replying;
  `:attach -m` uses `file-picker-cmd` (fzf/ranger).

## 7. Search, Filter, Threading

- `:search` highlights + `n/N`; `:filter` restricts the list to matches
  (successive filters AND together); `:clear` resets.
- IMAP/Maildir/JMAP share one flag language: terms (subject by default), `-b`
  body, `-a` all text, `-f/-t/-c` addresses, `-H header:value`, `-r/-u`
  read/unread, `-x/-X <flag>`, `-d since..until` with rich date syntax
  (ISO, `today`, weekday/month names, `1w1d` offsets). IMAP searches
  server-side; Gmail `-e` extension.
- notmuch takes **raw notmuch query language** in
  `:search`/`:filter`/`:cf`/`:query` — full-text, tags, boolean ops.
- **Threading**: client-side by References/In-Reply-To for any backend, uses
  **server-side IMAP THREAD** when available (`force-client-threads`
  override), optional `threading-by-subject`; dummy parents for orphans;
  `:fold`/`:unfold` (incl. `-a`), folded counts, `:toggle-thread-context`;
  per-folder sort + `reverse-msglist-order`.

## 8. Hooks and Shell Integration

- `[hooks]`: arbitrary shell commands with `$AERC_*` env context:
  `aerc-startup`, `aerc-shutdown`, `mail-received` (→ desktop notifications),
  `mail-added`, `mail-deleted`, `mail-sent`, `flag-changed`, `tag-modified`.
- `:pipe [-m|-p|-b|-s]` pipes raw mail to any command (classic:
  `:pipe -m git am -3` — multiple marked messages sorted in [PATCH n/m]
  order); `:exec` background commands; `:term` shells; `send-keys` scripts
  the embedded terminal; `:menu -c` glues fzf/dmenu onto any command; CLI IPC
  (`aerc :cf work/INBOX`, `aerc mailto:`).
- **`:patch` suite**: project tracking for git-email review — `init`,
  `apply` (optionally into a linked worktree), `drop`, `rebase`, `list`,
  `find <commit>`, `cd`, `term`, `switch`, `unlink`.

## 9. PGP

- `pgp-provider = auto | gpg | internal`: `gpg` shells out to system GnuPG
  (keyring, agent, smartcards work); `internal` is a built-in
  go-crypto/openpgp implementation with its own keyring;
  `use-terminal-pinentry` option.
- Per-account: `pgp-key-id`, `pgp-auto-sign`, `pgp-opportunistic-encrypt`
  (encrypt iff all recipients' keys available), `pgp-attach-key`,
  `pgp-self-encrypt`, `pgp-error-level`.
- Viewer shows signed/encrypted status with configurable icons and verifies
  signatures; `trusted-authres` for DKIM/ARC display.

## 10. Configuration Files

- `~/.config/aerc/`: **aerc.conf** (UI/behavior; optional — sane defaults),
  **binds.conf** (optional), **accounts.conf** (0600-enforced),
  `stylesets/`, `templates/`; CLI overrides `-C/-A/-B`; `:reload`
  hot-reloads.
- **accounts.conf** per account: `source`, `outgoing`, `from`, `aliases`,
  `default`, `archive`, `postpone`, `copy-to` (cross-account),
  `copy-to-replied`, `folders`/`folders-exclude`/`folders-sort`/`folder-map`,
  `check-mail`/`check-mail-cmd`, `signature-file`/`signature-cmd`,
  `headers`/`headers-exclude`, `subject-re-pattern`, `restrict-delete`,
  `strip-bcc`, `trusted-authres`, `pgp-*`; interactive `:new-account` wizard.
- **aerc.conf sections**: `[general]`, `[ui]` (+ contextual
  `[ui:account=X]`, `[ui:folder=Y]`, `[ui:folder~re]` overrides),
  `[statusline]`, `[viewer]` (pager, `alternatives` MIME preference order,
  `header-layout`, parse-http-links), `[compose]`, `[filters]`, `[openers]`,
  `[multipart-converters]`, `[hooks]`, `[templates]`.
- **Stylesets**: `object.attribute = value`; fg/bg/bold/italic/underline/
  dim/blink/reverse; colors by name, #hex, 0–255, or terminal default;
  objects for msglist (unread/read/flagged/deleted/marked/answered/result/
  gutter/pill/thread-folded/…), dirlist, statusline, tab, border, spinner,
  completion, and colorize-filter styles (quote_1..n, diff_add/del/meta,
  url, header, signature); `.selected` modifier; fnmatch wildcards;
  **dynamic per-message styling by header regex**
  (`msglist*.From,~^Bob.fg = blue`); `[user]` styles callable from
  templates; live-switchable with `:reload -s`.
- **Templating everywhere**: index columns, dirlist, statusline, tab titles,
  and compose bodies share one template system — data fields (.From, .To,
  .Subject/.SubjectBase, .Date, .OriginalText, .IsUnread/.IsFlagged/
  .HasAttachment, .ThreadCount/.ThreadPrefix, .Account, .Folder, …) and
  functions (wrap, quote, trimSignature, names/initials/emails, dateFormat,
  humanReadable, exec, switch/map/match, join/split, .Style…).

## 11. UX Strengths and Weaknesses (community consensus)

### Praised

- Works out of the box: wizard + sane vim defaults + good example configs;
  much quicker to set up than the mutt+offlineimap+msmtp+urlview stack.
- Fully **async**: flaky IMAP never freezes the UI (mutt's most-cited pain);
  fetches lazily; fast on huge folders.
- **Tabs + embedded terminal**: compose while reading; multiple accounts
  simultaneously; shells and git in-tab.
- Filters pipeline → pretty mail by default: colored quotes, highlighted
  diffs, HTML via sandboxed w3m, images, ICS invites.
- First-class **git-by-email** workflow (`:pipe git am`, `:patch`, hldiff).
- Discoverable: `:help` man pages in-app, review-screen key hints, readable
  INI config vs muttrc line-noise.
- Everything-is-a-command + macro keybindings + hooks + templates =
  mutt-grade scriptability with less arcana.

### Weaknesses

- Younger (2019); smaller ecosystem and feature surface than (neo)mutt.
- No POP3; no native address book (external command only); no local calendar
  integration (invite replies only); limited offline-first IMAP (users pair
  with mbsync/notmuch).
- Persistent `:filter` state confuses newcomers; mouse support is shallow.
- Some mutt power features absent or weaker: scoring, deep header-cache
  tuning, decades of edge-case handling, breadth of options.

## 12. The Best Parts — What Nitidus Should Take

1. **Embedded terminal instead of suspend**: editor/pager/filters as widgets;
   the UI stays live during compose — the #1 differentiator from mutt.
2. **Async-everything core** with lazy fetching and a worker-per-account
   model; the UI never blocks on the network.
3. **Tabs** as the universal container (accounts, viewers, composers,
   terminals).
4. **Keybindings = simulated keystrokes of ex-commands**: one command layer,
   trivially bindable/scriptable, context-scoped with inheritance.
5. **Filter pipeline** with a TTY-interactive option and theme-aware env —
   rendering delegated to composable external programs.
6. **Backend abstraction** spanning IMAP/JMAP/Maildir/notmuch/mbox behind one
   UI and one search grammar (with per-backend escape hatches).
7. **Templates everywhere** + layered stylesets with per-message
   header-regex styling.
8. **Review screen** before send, with live keybinding hints.
9. Hooks with env-var context; `:pipe`/`:menu`/`:choose` as shell glue;
   git-email patch tooling.
10. Sane defaults + in-app documentation (`:help`) — configurable, but never
    *requiring* configuration.

## Sources

- Homepage: <https://aerc-mail.org/>
- Man pages: aerc(1), aerc-config(5), aerc-accounts(5), aerc-binds(5),
  aerc-imap(5), aerc-maildir(5), aerc-notmuch(5), aerc-jmap(5), aerc-smtp(5),
  aerc-search(1), aerc-templates(7), aerc-stylesets(7), aerc-patch(7)
  (via man.archlinux.org)
- Repository: <https://git.sr.ht/~rjarry/aerc>
  (filters: <https://github.com/rjarry/aerc/tree/master/filters>)
- LWN: A look at the aerc mail client — <https://lwn.net/Articles/993498/>
- Community: HN discussions (33166054, 41321981), Terminal Trove
