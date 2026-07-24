# NeoMutt — Feature Analysis

Reference analysis of NeoMutt (neomutt.org) for the nitidus specification.
NeoMutt is the mutt fork that folds in the major community patchsets.
Compiled from the official guide, feature pages, and community discussion, as
the benchmark for what nitidus must match.

## 1. Mail Backends, Protocols, and Auth

### Local mailbox formats

- **Maildir** — one file per message; preferred modern local format; header
  caching supported.
- **mbox** — single-file mailbox; `mbox-hook` moves read mail out of spool.
- **MH** — one file per message with numbered filenames; header caching.
- **MMDF** — mbox variant with `^A^A^A^A` delimiters.
- **Compressed folders** — any of the above wrapped in gzip/gpg/anything via
  `open-hook`/`close-hook`/`append-hook` shell commands.

### Remote protocols

- **IMAP** — full client: folder browser, subscribe/unsubscribe,
  create/delete/rename mailboxes, server-side search (`=b`/`=B`/`=h`
  patterns, `=/` Gmail X-GM-RAW), IMAP keywords as tags. Synchronous/blocking
  (see §11).
- **POP3** — `pop://`/`pops://`; `<fetch-mail>` pulls into local spool; body
  caching.
- **SMTP** — built-in submission via `$smtp_url` (smtp/smtps, STARTTLS), or
  pipe to any sendmail-compatible command (`$sendmail`).
- **NNTP** — read/post Usenet news, newsrc handling (NeoMutt-added).
- **notmuch** — full virtual-mailbox backend over the notmuch/Xapian index
  (see §2).

### TLS / Auth

- SSL/TLS via OpenSSL or GnuTLS; `$ssl_starttls`, `$ssl_force_tls`, TLS-SNI,
  certificate prompting/pinning (`~/.mutt_certificates`).
- SASL: LOGIN, PLAIN, CRAM-MD5, GSSAPI/Kerberos, ANONYMOUS.
- **OAUTHBEARER and XOAUTH2** for IMAP/POP/SMTP — token supplied by external
  script (`$imap_oauth_refresh_command` etc.). No built-in OAuth flow.
- **`account` command** — populate credentials from an external command
  (password managers); `account-hook` for per-connection settings.
- **Caching**: header cache (lmdb, gdbm, bdb, kyotocabinet, qdbm, rocksdb,
  tdb, tokyocabinet backends; optional compression) + whole-body cache
  (`$message_cache_dir`) for IMAP/POP.

## 2. Index (Message List)

### Threading

- JWZ-style threading via References/In-Reply-To, optional subject-based
  pseudo-threading (`$sort_re`, `$strict_threads`); thread tree in the index.
- Collapse/expand threads, `$collapsed` state, `%M` collapsed-count expando.
- Thread ops: delete/undelete thread, tag thread, jump to parent, next/prev
  thread.
- Thread repair: `<break-thread>` and `<link-threads>`.
- **`use_threads`** — decouples threading from sort:
  `set use_threads=threads sort=last-date`.
- **`<limit-current-thread>`** — focus the view on one thread.

### Sorting

- `$sort`: date/date-received/from/to/subject/size/score/spam/threads/label/
  unsorted; `reverse-` prefix; `$sort_aux` secondary/within-thread sort
  (`last-` prefix for thread-latest).

### Pattern language

Used by limit, search, tag-pattern, hooks, scoring, coloring — one grammar:

- Header/content: `~f` from, `~t` to, `~c` cc, `~C` to|cc|bcc, `~K` bcc,
  `~e` sender, `~L` from|sender|to|cc, `~s` subject, `~b` body, `~B`
  body+headers, `~h` any header, `~i` message-id, `~x` references, `~M`
  content-type, `~y` X-Label, `~Y` tags, `~w` newsgroups, `~H` spam attr.
- Flags/state: `~A` all, `~N` new, `~O` old, `~U` unread, `~R` read, `~F`
  flagged, `~D` deleted, `~T` tagged, `~Q` replied, `~E` expired, `~S`
  superseded, `~v` collapsed-thread member, `~p` to-you, `~P` from-you,
  `~l` mailing list, `~u` subscribed list.
- Crypto: `~g` signed, `~G` encrypted, `~V` verified, `~k` contains PGP key.
- Numeric/date: `~m` msg number range, `~n` score, `~z` size, `~X`
  attachment count, `~d` sent date-range, `~r` received date-range
  (absolute, relative `>1w`, error-bounded `*`).
- Thread-relational: `~(pat)` thread contains, `~<(pat)` parent matches,
  `~>(pat)` child matches, `~#` broken, `~$` unreferenced, `~=` duplicates.
- Composition: `!` NOT, `|` OR, implicit AND, `()` grouping; `%` group-match
  variants; `=` exact-string (and IMAP server-side) variants.

### Tagging, scoring, colors, format

- **Tagging** — `t` tag message, `T` tag-pattern, `^T` untag-pattern, tag
  thread; `;` (tag-prefix) applies the next function to all tagged;
  `$auto_tag`.
- **Scoring** — `score PATTERN VALUE` (+ `unscore`); drives `$sort=score`,
  `$score_threshold_delete/_flag/_read`.
- **Color rules** — `color index PATTERN` whole-line, plus per-column
  index-color: `index_author`, `index_subject`, `index_date`, `index_flags`,
  `index_number`, `index_size`, `index_label`, `index_collapsed`,
  `index_tag(s)` — each pattern-driven.
- **index_format** — printf-style expandos (`%Z` flags, `%F` author, `%s`
  subject, `%d`/`%[fmt]` dates, `%l`/`%c` size, `%g`/`%J` tags…),
  width/justify modifiers, `%<expando?then&else>` nested conditionals,
  soft-fill `%*`, padding; **cond-date** idiom via `index-format-hook`;
  `%@name@` custom expandos.

### notmuch virtual mailboxes

- `virtual-mailboxes "Inbox" "notmuch://?query=tag:inbox"` — named
  saved-search folders; Xapian query syntax (`tag:`, `from:`, `date:..`,
  and/or); `type=threads|messages`.
- Functions: `<change-vfolder>`, `<vfolder-from-query>` (tag completion),
  `<entire-thread>` (pull whole thread into view), `<modify-labels>`,
  `<modify-labels-then-hide>`, rolling query windows
  (`<vfolder-window-forward/backward/reset>`).
- Tags shown/edited in index, map to maildir flags; **custom tags** also
  backed by IMAP keywords; `tag-transforms`/`tag-formats` for display.

### Other index niceties

Quasi-delete (hide without deleting), trash folder (`$trash`),
`new-mail-command` (external notification), progress bar, fuzzy search in
menus, `mark-message` bookmarks.

## 3. Pager

- **Header weeding** — `ignore`/`unignore` lists; `header-order`; `$weed`.
- Internal pager: `$pager_index_lines` mini-index above, `$pager_context`,
  `$pager_stop`, `$smart_wrap`, `$wrap`/`$reflow_*` (format=flowed),
  `$markers`, `$tilde`.
- **Colors/regex highlighting** — `color body REGEX`, `color header REGEX`,
  quote levels `quoted`..`quoted9` via `$quote_regex`; `color signature`;
  **attach-headers-color**; 256-color + `#RRGGBB` + attributes.
- Quoted-text handling: toggle quoted, **skip-quoted**,
  `$toggle_quoted_show_levels`.
- **Mailcap** — full RFC 1524: `test=`, `copiousoutput`, `needsterminal`,
  `nametemplate`, `%s`/`%t`/`%{param}` expansion.
- **auto_view** — render MIME types inline via copiousoutput mailcap entries;
  **text/html read via `lynx -dump`/w3m in mailcap** — no native HTML
  rendering (see §11).
- `alternative_order` picks the preferred multipart/alternative part.
- Attachment menu: view/pipe/save/print attachments, per-type via mailcap.
- **pager_read_delay** — preview without immediately marking read.
- URL handling delegated to external `urlview`/`extract_url` via macro.
- Inline PGP decrypt/verify in pager; encryption-info block display.

## 4. Compose

- **External editor is the composer** — `$editor` launched on a temp file;
  `$edit_headers` puts To/Cc/Bcc/Subject in the editor buffer.
- **Compose menu** (post-editor staging screen): re-edit any header, attach
  files or messages, edit/re-order/delete attachments, per-attachment
  content-type/encoding/description/disposition, rename, filter through
  command, ispell.
- **Compose preview** — body preview pane in the compose dialog.
- **Crypto menu** — PGP / S/MIME / autocrypt: sign, encrypt, sign-as, both,
  opportunistic mode, clear; colored security status.
- **Postpone/drafts** — writes to `$postponed`; auto-offered on next compose
  or `neomutt -p`; `$postpone_encrypt`.
- **Fcc** — save-copy prompt, `$record`, `fcc-hook` pattern-routing,
  multiple comma-separated Fcc mailboxes.
- Reply machinery: reply, group-reply, list-reply, `$reverse_name`,
  `$include` quoting with `$attribution_intro`, forward inline vs
  `$mime_forward`, bounce, resend, compose-to-sender, Mail-Followup-To,
  X-Original-To reply.
- **Forgotten-attachment check** — `$abort_noattach_regex` warns if you
  mention an attachment but attach nothing.
- Batch/CLI sending: `neomutt -s subj -a file -- addr`, `-H` draft, `-C`
  batch crypto.
- `send-hook`/`send2-hook`/`reply-hook` mutate settings per recipient
  mid-compose; DSN; `$abort_unmodified`, `$abort_nosubject`.

## 5. Crypto

- **PGP via GPGME** (recommended) or classic mode driving gpg through
  `pgp_command_*` format strings; PGP/MIME (RFC 3156) and traditional inline
  PGP.
- **S/MIME** via GPGME/gpgsm, or classic openssl mode + `smime_keys` store.
- Key variables: `$pgp_default_key`, `$pgp_sign_as`, `$crypt_autosign`,
  `$crypt_autoencrypt`, `$crypt_replysign`, `$crypt_replyencrypt`,
  `$crypt_opportunistic_encrypt` (auto-encrypt when all keys known),
  `$crypt_verify_sig`.
- `crypt-hook PATTERN KEYID` — force key per recipient.
- Key-selection menus with trust display; gpg-agent/pinentry passphrase
  handling.
- **Autocrypt** (GPGME + SQLite3): per-address accounts, dedicated ECC
  keyring + peer DB, header parsing + gossip keys, recommendation engine in
  compose, account menu.
- Crypto state searchable via patterns (`~g ~G ~V ~k`) and flagged in index.

## 6. Address Book

- **Aliases** — `alias key addr...` with comments and tags; alias menu;
  `$alias_file` + `<create-alias>` appends learned addresses; groups;
  Tab completion; alias patterns filter the menu.
- **`$query_command`** — external directory search (abook, khard,
  notmuch-addrlookup, lbdb, LDAP); protocol: status line, then TSV
  `address<TAB>name<TAB>extra`; query menu + inline complete-query.
- **Gap: no native contact manager** — no built-in storage, CardDAV sync,
  editing UI, or auto-harvesting beyond `create-alias`; ecosystem norm is
  abook/khard + lbdb glue. **Nitidus differentiates here.**
- Related: `group`/`ungroup` named address groups usable in patterns and
  hooks; `lists`/`subscribe` mailing-list identity; `$reverse_alias`.

## 7. Hooks (complete list)

| Hook | Purpose |
|---|---|
| `folder-hook` | Run config commands before entering a matching mailbox |
| `account-hook` | Run commands whenever a remote connection is used |
| `mbox-hook` | Auto-move read mail from matching spool to another mailbox |
| `send-hook` | Adjust settings when recipients/pattern match, before composing |
| `send2-hook` | Like send-hook but re-evaluated after every edit |
| `reply-hook` | Like send-hook but only for replies |
| `message-hook` | Run commands before displaying a matching message |
| `fcc-hook` | Choose the Fcc mailbox by recipient pattern |
| `save-hook` | Default save/copy folder by pattern |
| `fcc-save-hook` | Sets both fcc and save defaults at once |
| `crypt-hook` | Force PGP/S-MIME key ID for a matching recipient |
| `open-/close-/append-hook` | Compressed-mailbox handling |
| `charset-hook` / `iconv-hook` | Charset aliasing |
| `index-format-hook` | Pattern-selected format snippets (cond-date) |
| `startup-hook` / `shutdown-hook` / `timeout-hook` | Global lifecycle hooks |
| `new-mail-command` (var) | Shell command on new-mail arrival |

Semantics: hooks are sticky (settings persist until another hook changes
them — hence the `folder-hook . 'set ...'` default-reset idiom); pattern
hooks use the pattern language, regex hooks use regex.

## 8. Keybindings, Macros, Functions

- `bind MENU[,MENU...] KEY FUNCTION` — per-menu maps; menus: generic, alias,
  attach, browser, editor, index, compose, pager, pgp, smime, postpone,
  query, autocrypt (+ sidebar functions in index/pager); `generic` is the
  fallback for all except pager/editor.
- Key syntax: `\Cx`, `<esc>x`, `<f1>`–`<f10>`, `<tab>`, `<enter>`;
  `<noop>` unbinds; a few fallback keys are protected.
- `macro MENU KEY "SEQUENCE" "description"` — mixes literal keys, named
  `<function>` calls, prompts, `:` config commands, shell pipes — NeoMutt's
  whole automation story (e.g. `macro index \cb "|urlview\n"`).
- **Named functions** — every operation is a `<function>` (~200+ across
  menus); `push` feeds sequences at startup; `exec` runs functions by name;
  `?` shows current bindings.
- The line editor has its own bindable map with separate histories for
  commands, addresses, files, patterns.

## 9. Config System

- **neomuttrc** command language: `set`/`unset`/`toggle`/`reset` (bool, quad
  `ask-yes`, number, string, enums), `source` includes (with backtick shell
  substitution), `ifdef`/`ifndef`, `my_` user variables, `-e` CLI overrides.
- **~800 documented config variables** (up from mutt's ~300).
- `mailboxes` (with `-label`, `-poll`, `-notify`) + `named-mailboxes` feed
  new-mail polling, browser, and sidebar; `virtual-mailboxes` for notmuch.
- **Sidebar**: visibility/width/format/indent options, `new_mail_only`,
  on-right, divider, sort modes; next/prev(-new)/open/toggle/search
  functions; pin/unpin; 9 color objects.
- **Colors**: named + 256 + `#RRGGBB`, attributes, `default` transparency;
  objects for every UI element (status, indicator, tree, markers, prompt,
  progress, compose_*, sidebar_*, quoted0-9, index-*, body/header regex).
- Format strings everywhere: `$index_format`, `$status_format`,
  `$pager_format`, `$folder_format`, `$attach_format`, `$compose_format`,
  `$sidebar_format` — one shared expando/conditional/padding mini-language.
- Misc: `spam`/`nospam` (extract spam scores into a matchable
  pseudo-header), `subjectrx` (display-rewrite subjects), `alternates`,
  `attachments` (what counts for `%X`/`~X`), `mime-lookup`, `unhook`,
  `setenv`; experimental embedded **Lua scripting**.

## 10. Notable NeoMutt-Specific Features (beyond mutt)

Sidebar (search, pinning, theming) · notmuch integration (virtual folders,
tags, entire-thread, query windows) · compressed folders · encrypted mbox ·
index-color per-column theming · attach-headers-color · cond-date +
nested-if format conditionals · custom tags backed by notmuch or IMAP
keywords · use_threads · limit-current-thread · compose preview ·
compose-to-sender · forgotten-attachment warning · multiple Fcc · pluggable
header-cache backends + compression · NNTP · `account` command ·
OAUTHBEARER/XOAUTH2 · TLS-SNI · global hooks (startup/shutdown/timeout) ·
new-mail command · ifdef config · Lua scripting · trash folder ·
quasi-delete · skip-quoted · status-color · progress bar · fuzzy search ·
pager read delay · initials expando · encrypt-to-self · encryption-info ·
command-line crypto (-C) · reply-with-X-Original-To.

## 11. Commonly Cited Pain Points

- **Synchronous, blocking architecture** — UI freezes during IMAP
  fetch/sync/large downloads; single-threaded core acknowledged by
  maintainers as hard to fix (GitHub #1081, #1509); large IMAP folders take
  minutes without header cache.
- **Config complexity / learning curve** — nothing works out of the box; a
  real setup needs multi-file muttrc + mailcap + mbsync/offlineimap + msmtp
  + notmuch + abook glue.
- **Multi-account is DIY** — folder-hook/account-hook juggling with
  sticky-settings footguns, vs aerc's native accounts + tabs.
- **HTML email is hostile** — mailcap + lynx/w3m dumps, no inline
  images/links; a top defection reason in a mostly-HTML email world.
- **OAuth2 requires external scripts** — painful with Gmail/O365.
- **Arcane defaults and legacy baggage** — hundreds of variables, printf
  mini-languages, order-sensitive sticky hooks.
- **No native address book**, no calendar/ICS handling, weak attachment
  previews — all outsourced.
- **Sending is blocking** — no background send/undo-send queue without
  wrappers.

## 12. The Best Parts — What Nitidus Must Match

- **The pattern language** — one grammar for
  limit/search/tag/score/color/hooks; unmatched anywhere else.
- **notmuch virtual folders** — instant full-text search over huge archives,
  saved-search mailboxes, tag workflows.
- **Macros over everything** — keystroke automation composing functions +
  shell pipes; whole workflows as one key.
- **Hooks** — context-sensitive reconfiguration (per-folder, per-recipient,
  per-account) with zero UI.
- **External editor composing** (vim as first-class composer) +
  `edit_headers`.
- **Speed with local Maildir** — keyboard-only triage of thousands of
  messages; header caching.
- **Threading quality** — proper JWZ threading, thread surgery
  (break/link), collapse, thread-scoped ops.
- **Total theming/format control** — index_format + per-column colors +
  regex body highlighting.
- **git-send-email adjacency** — patch workflows, list-reply,
  Mail-Followup-To handling.
- **25 years of muttrc corpus** — every problem has a documented recipe.

## Sources

- NeoMutt guide: <https://neomutt.org/guide/> (Getting Started,
  Configuration, Advanced Usage, MIME Support, Optional Features, Autocrypt,
  Reference, Security)
- Feature pages: <https://neomutt.org/feature.html> (Sidebar, Notmuch,
  Compressed Folders, Index Color)
- <https://docs.neomutt.org> (encryption explanation, S/MIME how-to)
- Pain points: NeoMutt GitHub issues #1081, #1509, #4411; aerc reviews
  (splint.rs, wilw.dev); notmuchmail.org/mutttips
