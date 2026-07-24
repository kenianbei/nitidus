# Gmail — Feature Analysis

Reference analysis of Gmail for the nitidus specification, focused on (a) the
UI/workflow features that shape the expectations of users migrating from
Gmail to a TUI, and (b) the protocol behavior nitidus must handle when
syncing Gmail accounts over IMAP.

## 1. The Mental Model: Labels, All Mail, Conversations

### Labels, not folders

- A message carries **any number of labels simultaneously**; labels are tags,
  not containers. "Moving" is removing one label and adding another.
- **Nested labels** (`Work/Projects/Alpha`) are display-only sugar — the
  child is an independent label whose name contains `/`.
- Labels have per-label colors, visibility settings (show/hide,
  show-if-unread), and per-label **"Show in IMAP"** toggles.
- **System labels**: Inbox, Starred, Snoozed, Important, Sent, Drafts, Spam,
  Trash, All Mail, plus category labels. "In the inbox" = has the `Inbox`
  label.
- **All Mail** is the universal archive: every message not in Spam or Trash
  lives there, always.

### Archive-first workflow

- **Archive** = remove the `Inbox` label; the message stays in All Mail and
  keeps other labels. The core Gmail habit: inbox is a to-do queue, not a
  filing cabinet; archive fearlessly, retrieve by search.
- Gmail users largely don't file — they archive and search. Deleting is
  rare; Trash and Spam auto-purge after 30 days.

### Conversation view

- Threads grouped by Gmail's own algorithm (subject sans Re:/Fwd: +
  references + time window; capped at 100 messages per thread).
- **Sent replies appear inline in the thread** — a major expectation.
- Thread rows show participant summary ("Alice, Bob, me (5)"), snippet, and
  label chips; older messages collapse; quoted text folds behind a toggle.
- Conversation view can be disabled; thread-list actions apply to the whole
  conversation.

## 2. Inbox Organization

- **Category tabs**: Primary, Social, Promotions, Updates, Forums —
  auto-classified server-side; drag-to-retrain per sender. Exposed to
  search as `category:` but **not as IMAP folders**.
- **Importance markers** (yellow arrow): trainable ML "important to you";
  `+`/`-` to train; hoverable explanation.
- **Inbox types**: Default (tabs), Important first, Unread first, Starred
  first, Priority Inbox (sections: important-and-unread / starred /
  everything else), **Multiple Inboxes** — up to 5 extra panes, each defined
  by an arbitrary search query (saved-search smart mailboxes on one screen).
- **Stars / superstars**: default yellow star expandable to 12 icons; `s`
  cycles; each searchable (`has:yellow-star`, `has:red-bang`). IMAP sees
  only `\Flagged` — superstar identity is lost.
- **Snooze** (`b`): leaves the inbox, returns at a chosen time; lives under
  the Snoozed label. Not representable over IMAP.
- **Nudges**: auto-resurfaced "Received 3 days ago. Reply?" reminders.
- Furniture: unread counts per label, reading pane (off/right/below),
  density modes, row hover-actions, per-tab new-mail badges.

## 3. Search

Search is Gmail's substitute for filing; operator fluency is high.

| Operator | Meaning |
|---|---|
| `from:` `to:` `cc:` `bcc:` | Address fields |
| `deliveredto:` | Delivered-To header (alias detection) |
| `subject:`, `"exact phrase"`, `+word` | Subject, phrase, exact word |
| `OR` / `{a b}`, `-term`, `( )` | Boolean composition |
| `AROUND n` | Proximity search |
| `label:`, `has:userlabels` / `has:nouserlabels` | Labels |
| `category:primary\|social\|promotions\|updates\|forums…` | Tabs |
| `in:inbox\|archive\|snoozed\|sent\|draft\|spam\|trash\|anywhere` | Location |
| `is:unread\|read\|starred\|important\|muted` | State |
| `has:attachment`, `filename:pdf` | Attachments |
| `has:drive\|document\|spreadsheet\|presentation\|youtube` | Rich links |
| `has:yellow-star`, `has:red-bang`, … | Specific superstar |
| `size:` / `larger:` / `smaller:` | Size (`larger:10M`) |
| `after:` `before:` `older:` `newer:` | Absolute dates |
| `older_than:` / `newer_than:` | Relative (`newer_than:2d`, d/m/y) |
| `list:` | Mailing list |
| `rfc822msgid:` | Exact Message-ID lookup |

- **Search chips** refine results interactively (From, Any time, Has
  attachment, Is unread…) — casual users filter via chips, not operators.
- The advanced-search form converts directly into a **filter** ("Create
  filter") — search and rules share one grammar.
- Full-text over bodies and inside many attachments, fast at mailbox scale;
  whole-word/stem matching only, no regex.

## 4. Keyboard Shortcuts

Gmail's opt-in shortcut system descends from mutt/vi conventions — Gmail
power users map onto a TUI almost 1:1. The load-bearing set:

- **Navigation**: `j`/`k` list movement, `o`/`Enter` open, `u` back to
  list, `n`/`p` messages within a conversation, `/` search.
- **Go-to chords**: `g i` inbox, `g s` starred, `g b` snoozed, `g t` sent,
  `g d` drafts, `g a` all mail, `g l` label (autocomplete).
- **Selection**: `x` toggle-select; `* a` all, `* n` none, `* r` read,
  `* u` unread, `* s` starred, `* t` unstarred.
- **Actions**: `e` archive, `#` delete, `!` spam, `m` mute, `b` snooze,
  `s` star-cycle, `Shift+i`/`Shift+u` read/unread, `+`/`-` importance,
  `l` label menu, `v` move menu, `[`/`]` **archive-and-advance**,
  `z` **undo last action**, `;`/`:` expand/collapse conversation.
- **Compose**: `c` compose, `r` reply, `a` reply-all, `f` forward,
  `Ctrl+Enter` send, `Ctrl+Shift+c/b` Cc/Bcc, `Ctrl+Shift+f` switch From.
- **Meta**: `?` shortcut cheat-sheet overlay; every shortcut remappable.

## 5. Compose

- **Undo Send**: configurable 5/10/20/30-second send delay with an "Undo"
  toast — a delay, not recall; trivially replicable in a TUI.
- **Schedule send**: future date/time; scheduled mail is cancelable/editable
  before sending (up to 100).
- **Smart Compose** (ghost-text completion) and **Smart Reply** (three
  suggested replies) — AI-tier, out of TUI scope.
- **Confidential mode**: expiring, non-forwardable messages; over IMAP both
  sides see only a "view the message" link placeholder — render gracefully.
- **Templates**: saved bodies, insertable and usable as filter auto-replies.
- **Signatures**: multiple, per send-as alias, separate new-vs-reply
  defaults.
- **Send-as aliases**: send from verified addresses; "reply from the address
  it was sent to" option; plus-addressing and dot-insensitivity.
- Attachment-forgotten warning; continuous draft autosave (visible over
  IMAP, with duplicate-draft quirks); vacation responder is server-side.

## 6. Triage Workflows

- **Batch operations**: selection + one-keystroke actions;
  select-all-matching-search for mailbox-scale sweeps.
- **"Filter messages like these"**: pre-fills a filter from an exemplar
  message — the canonical "never see this again" flow.
- **Mute** (`m`): thread auto-archives all future replies; `is:muted`.
- **Block sender** (→ Spam) and one-click **Unsubscribe** (List-Unsubscribe
  mailto / RFC 8058 one-click HTTP), plus "you never open these" prompts.
- **Report spam/phishing**; moving out of Spam trains the filter.
- **Auto-advance**: after archive/delete go to next conversation instead of
  back to the list — pairs with `[`/`]`.
- **Undo everywhere**: archive/delete/label/move/send all get a transient
  Undo (`z`).

## 7. Filters / Rules

- **Criteria**: From, To, Subject, Has the words (accepts the full search
  grammar), Doesn't have, Size, Has attachment.
- **Actions**: skip inbox (archive), mark read, star, apply label,
  categorize, forward, delete, **never send to Spam**, always/never mark
  important, send template. Retroactive apply on creation.
- **Import/export**: mailFilters XML (Atom) file — a parseable format worth
  supporting for one-time import of a migrating user's rules.
- Filters run **server-side on arrival** — an IMAP client inherits their
  effects (mail arrives pre-labeled/archived); the right division of labor.

## 8. Attachments and Rich Content

- Inline images; HTML rendered fully with remote images loaded through
  Google's proxy by default ("ask first" option exists).
- Attachment chips on rows and messages; hover to save without opening.
- Built-in previews for PDFs/Office/images/video; Drive integration
  (>25 MB auto-converts to Drive links; `has:drive` finds them).
- Calendar invites rendered inline with RSVP buttons; `.ics` expected.
- Send limit 25 MB, receive 50 MB; executable attachment types blocked.

## 9. Protocol Access: IMAP Specifics

### Layout and semantics

- Each label = one IMAP folder; system labels under **`[Gmail]/`**
  (`All Mail`, `Sent Mail`, `Drafts`, `Trash`, `Spam`, `Starred`,
  `Important`); `[Gmail]` itself is `\Noselect`; non-ASCII labels in UTF-7.
- **RFC 6154 SPECIAL-USE** attributes always returned (`\All`, `\Sent`,
  `\Drafts`, `\Trash`, `\Junk`, `\Flagged`, nonstandard `\Important`) —
  identify roles by attribute, never by name.
- **One message, many folders**: a message with 3 labels appears in 3
  folders + All Mail. Naive clients download it N times and show
  duplicates — deduplicate via `X-GM-MSGID`.
- COPY to a folder = add label. **Archive** = remove from `INBOX` (stays in
  All Mail). **Real deletion** = move to `[Gmail]/Trash` (strips all labels,
  purges in 30 days). Per-label "Show in IMAP" means special folders may be
  absent.

### X-GM-EXT-1 extensions (the Gmail superpowers)

- **`X-GM-MSGID`** — immutable 64-bit id, stable across folders (dedup key).
- **`X-GM-THRID`** — Gmail's thread id: reproduces **Gmail's exact
  conversation grouping** instead of References threading.
- **`X-GM-LABELS`** — FETCH the full label set; STORE `+/-X-GM-LABELS`
  adds/removes labels **without leaving the current folder** — the key to
  first-class multi-label support.
- **`X-GM-RAW`** — `UID SEARCH X-GM-RAW "…"` runs the **full web search
  grammar server-side** — closes the search-quality gap entirely.
- Standard capabilities present: IDLE, UIDPLUS, CONDSTORE, ESEARCH,
  XOAUTH2.

### Flags, settings, quirks

- `\Seen` read; `\Flagged` = any star (superstar identity lost);
  Important is a label, not a flag; arbitrary IMAP keywords are not how
  labels work — use X-GM-LABELS.
- **Auto-Expunge ON (default)**: marking `\Deleted` expunges instantly.
  OFF: client controls timing + a setting for what expunge-from-last-folder
  means (*Archive* [default] / *Trash* / *delete forever*). Nitidus must
  never assume delete semantics — probe/document these settings.
- **Folder size limits**: user setting can cap each IMAP folder at
  1,000–10,000 recent messages (mysteriously truncated folders).
- Sending via Gmail SMTP **auto-saves to Sent Mail** — do not also APPEND
  or you create duplicates. Gmail rewrites From to a verified address.
- 15 simultaneous connections max; bandwidth caps; avoid syncing All Mail
  *and* every label folder in full.

### Authentication

- Basic auth is dead (personal 2024, Workspace 2025). Options:
  - **OAuth 2.0 (XOAUTH2)** — Google Cloud app registration, scope
    `https://mail.google.com/`; unverified apps face consent friction and
    a 100-user cap.
  - **App passwords** — 16-char, only with 2-Step Verification enabled;
    the pragmatic path for most TUI users today.
- The Gmail REST API is the alternative access path (labels as objects,
  history-based delta sync, push via Pub/Sub, filter/settings endpoints) —
  out of scope for nitidus's MVP but noted for a future backend.

## 10. What Gmail Users Miss in IMAP Clients

1. **Labels flattened to folders** — multi-label becomes "which folder?";
   label chips disappear.
2. **Duplicate messages** across label folders + All Mail.
3. **Search quality gap** — the #1 complaint; `X-GM-RAW` passthrough is the
   known fix but few clients use it.
4. **Threading differences** vs Gmail's grouping; sent mail not interleaved;
   `X-GM-THRID` fixes it but is rarely used.
5. **Archive awkward or dangerous** — no single-key archive; fear of
   expunge settings destroying mail.
6. Server-side features that don't traverse IMAP: snooze, undo send,
   schedule send, categories/tabs, superstars, mute, nudges, one-click
   unsubscribe.
7. Sync pain at scale (decade-deep All Mail, connection caps).
8. Auth friction (OAuth verification, app-password discovery).
9. Sent-mail duplication and draft multiplication.
10. Filters live server-side only — users keep the web UI around to edit
    them.

## 11. The Best Parts — What Nitidus Should Imitate

1. **The shortcut grammar wholesale**: `j/k/x/e/#/r/a/f/s/!/m/u/z`,
   `g`-prefix goto chords, `*`-prefix bulk selection, `[`/`]`
   archive-and-advance, `?` help overlay, `l`/`v` fuzzy label/move pickers.
   Gmail's keys came from mutt — bring them home.
2. **Archive as the primary, single-key, safe verb** (`e`), distinct from
   delete, with All Mail as the safety net.
3. **Universal undo (`z`)** for every triage action + **undo send** via a
   local 5–30 s outbound delay.
4. **True labels**: multiple chips per message, add/remove without
   "moving," round-tripped via `X-GM-LABELS`; dedup by `X-GM-MSGID`.
5. **Gmail-fidelity threading** via `X-GM-THRID` when the backend is Gmail,
   with sent messages interleaved.
6. **Server-side search passthrough**: accept Gmail operator syntax
   verbatim, ship via `X-GM-RAW`; layer local refinement on top.
7. **Saved-search virtual mailboxes** ≈ Multiple Inboxes / Priority Inbox
   sections.
8. **Conversation-first rows**: participants + count + snippet + label
   chips + attachment icon; quoted-text folding in the pager.
9. **Snooze** (local hide-until-T), **mute**, **auto-advance** after triage.
10. **Sweep flows**: select-by-predicate, "filter messages like these" →
    generate a rule from an exemplar, one-key unsubscribe via
    List-Unsubscribe.
11. **Send-as aliases with per-alias signatures**, reply-from-delivered-to
    address, schedule-send.
12. **Safety rails around expunge semantics**: archive means
    remove-from-INBOX, never auto-expunge into Trash; document the Advanced
    IMAP Controls users should set.

## Sources

- Gmail IMAP extensions (X-GM-EXT-1):
  <https://developers.google.com/workspace/gmail/imap/imap-extensions>
- IMAP/SMTP overview: <https://developers.google.com/gmail/imap>
- XOAUTH2: <https://developers.google.com/workspace/gmail/imap/xoauth2-protocol>
- Search operators: <https://support.google.com/mail/answer/7190>
- Keyboard shortcuts: <https://support.google.com/mail/answer/6594>
- IMAP setup: <https://support.google.com/mail/answer/78892>
- Less-secure-apps shutdown: <https://support.google.com/a/answer/14114704>
- Migration complaints: Mozilla support/bugzilla threads on Gmail
  labels/duplicates over IMAP
