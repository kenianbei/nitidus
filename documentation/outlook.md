# Outlook — Feature Analysis

Reference analysis of Microsoft Outlook (new Outlook for Windows / Outlook
on the web, and the Exchange Online / Outlook.com service behind them) for
the nitidus specification. Focused on (a) UI/workflow features that shape
the expectations of users migrating from Outlook to a TUI, and (b) protocol
behavior nitidus must handle when syncing Outlook.com / Microsoft 365 mail.

## 1. Mental Model

- **True folders, not labels.** A message lives in exactly one folder
  (MAPI/Exchange model); moving is a real move; arbitrary-depth hierarchy.
  Well-known folders: Inbox, Drafts, Sent Items, Deleted Items, Junk Email,
  Archive, Outbox, Notes, Conversation History.
- **Categories = the label-like layer.** Named, colored tags; any number
  per message, orthogonal to folder location; shared across mail, calendar,
  contacts, tasks (deep-dive §8).
- **Flag + follow-up**: flagged/complete/none states plus optional
  start/due/reminder dates; flagged mail surfaces in To Do / My Day.
- **Pinned messages**: pin glues a message to the top of its folder until
  unpinned; server-side, Outlook-clients-only.
- **Focused/Other**: the Inbox is one folder presented as two tabs driven by
  per-message classification (unlike Gmail's label-based tabs).
- **Importance** (high/normal/low) set at compose time and shown in lists.
- **Archive is one folder**: one-click/one-key (`E`) move to a single
  Archive folder — simpler than Gmail's model.
- **Deleted Items + dumpster**: delete moves to Deleted Items; purged items
  sit in "Recoverable Items" (~14–30 days) — not visible over IMAP.

## 2. Inbox Organization

- **Focused Inbox**: ML classification into Focused/Other from contact
  graph + behavior; trained via "Move to Focused/Other" and per-sender
  "Always move" overrides; can be disabled; notifications can be
  Focused-only.
- **Conversation view**: optional; grouping by Exchange `ConversationId`
  (subject + heuristics), *not* strict RFC 5322 threading; newest on
  top/bottom; deleted/sent items optionally shown inside the conversation.
- **Pin to top**: per-folder pinned section that survives new mail.
- **Snooze**: hide and resurface at a chosen time (server-side, hidden
  scheduled folder, no public API).
- **Sweep** — the signature bulk-hygiene tool, per sender: move/delete all
  existing; delete all **and future** (ongoing rule); keep only the latest;
  delete older than 10 days. Managed under Settings > Sweep; runs
  server-side; distinct from normal rules. Extremely popular.
- **Junk handling**: Exchange Online Protection upstream; safe/blocked
  sender lists; report phishing/junk. (Clutter is dead — retired 2020.)
- **Quick actions on hover**: configurable per-row buttons (archive,
  delete, flag, pin, snooze, read/unread).
- **Sort/filter bar**: per-folder filter (unread, flagged, to me, has
  attachments, mentions me) and sort (date, from, size, importance).
- **Subscriptions manager** + inline Unsubscribe (List-Unsubscribe).

## 3. Rules, Quick Steps, Conditional Formatting

- **Server-side inbox rules** (run regardless of client): AND-ed conditions
  (from/to, subject/body keywords, attachment, importance, size, my name in
  To/Cc, …), actions (move/copy/delete, permanently delete, pin, mark read,
  set importance, categorize, forward/redirect), exceptions, explicit
  ordering, "stop processing more rules"; ~256 KB total rules budget.
- **Quick Steps**: user-defined one-click multi-action macros ("Move to X +
  mark read", "Reply & delete", "Flag + categorize + move") — the TUI
  equivalent is named command macros bound to keys.
- **Conditional formatting** (classic Outlook only): color/style message
  list rows by rule — heavily missed in new Outlook; a natural TUI feature.

## 4. Search

- **KQL-ish operators**: `from:`, `to:`, `cc:`, `participants:`,
  `subject:`, `body:`, `hasattachment:yes`, `attachment:name.pdf`,
  `isflagged:yes`, `isread:no`, `importance:high`,
  `category:"Red category"`, `received:`/`sent:` with dates and ranges
  (`received>=2026-01-01`, `received:yesterday`), `size:>5MB`, `kind:`,
  `folder:`, uppercase `AND`/`OR`/`NOT`, quotes, parentheses, `*` suffix
  wildcards.
- Scope: current folder / mailbox / all mailboxes; ranked "Top results"
  then chronological.
- **Search Folders** — the highlight: virtual folders defined by persistent
  criteria, living under a "Search Folders" node, updating continuously.
  Built-in templates (Unread, Flagged, From specific people, Large mail,
  Categorized) + fully custom. Stored server-side as Exchange objects;
  exposed via Graph as `mailSearchFolder`. **This is exactly a TUI virtual
  mailbox / notmuch saved search** — the most direct feature translation.
- Known limits: search folders don't span mailboxes; new Outlook's criteria
  editor is thinner than classic's.

## 5. Keyboard Shortcuts

- **Scheme switcher**: Outlook web offers **Outlook, Gmail, Yahoo, or off**
  shortcut schemes — precedent for nitidus shipping selectable keymaps.
- Key Outlook-mode keys: `?` help overlay, `/` search, `C`/`N` compose,
  `R` reply, `A` reply-all, `F` forward, `E`/`Backspace` archive, `Delete`
  delete, `Q`/`U` read/unread, `Insert` flag, `V` move to folder, `.`
  actions menu, **`Z`/`Ctrl+Z` undo (undoes moves/deletes!)**, `Ctrl+.`/
  `Ctrl+,` next/prev item.
- Classic desktop muscle memory: `Ctrl+Enter` send, `Ctrl+Shift+M` new
  mail, `Ctrl+Shift+V` move, `Ctrl+Shift+G` flag dialog.

## 6. Compose

- **@mentions**: `@name` highlights, auto-adds to To:, sets a "mentioned"
  property filterable as "@ mentions me"; rendered as a link in HTML.
- **Scheduled send**: server-side hold in Drafts until the chosen time;
  editable/cancelable.
- **Undo send**: configurable 0–10 s client-side delay before submission
  (distinct from Exchange "recall," which is org-only and unreliable).
- **Signatures**: multiple named HTML signatures; separate new-vs-reply
  defaults; roamed in the mailbox (cloud signatures).
- **My Templates / Quick Parts**: saved snippets inserted on click.
- **Attachment reminder**: body says "attached" + no attachment → warning.
- **Reply-all guardrails**: MailTips (large audience, external recipients,
  OOF recipients, "you were Bcc'd"), external-recipient highlighting,
  tenant reply-all storm protection.
- Other expectations: HTML default, inline images, OneDrive share-as-link
  vs attach, read/delivery receipts, sensitivity labels (business),
  Purview-encrypted mail that IMAP clients receive as portal-link mail,
  send-from-alias, per-message plain-text toggle.

## 7. Calendar / Meeting Integration (the polite-rendering bar)

- Invites are **iTIP/iMIP**: `text/calendar; method=REQUEST` MIME part.
  Outlook renders inline Accept / Tentative / Decline / Propose-new-time
  with a conflict-preview agenda; responses generate `method=REPLY`;
  cancellations arrive as `method=CANCEL`; updates as bumped-sequence
  REQUESTs.
- **Quirk**: accepting auto-deletes the invite from the Inbox by default —
  messages "vanish" in IMAP clients.
- **Minimum TUI bar**: parse and display text/calendar (organizer, local
  time, recurrence, attendees, sequence), send well-formed iMIP REPLY for
  accept/tentative/decline, recognize CANCEL and stale invites.
- **Voting buttons**: Exchange-proprietary/TNEF; over IMAP they arrive
  inert (often `winmail.dat`) — detect and name at most. Note TNEF
  decoding generally.
- Scheduling polls / share-availability arrive as ordinary HTML mail.

## 8. Categories Deep-Dive (the Gmail-labels analogue)

- **Master category list** per mailbox: name + color (25 presets classic,
  wider palette in OWA); default "Red/Blue/…" names users rename;
  server-side and roaming.
- Per-message: zero or more; colored chips in list and reading pane;
  assignable via UI, rules, Quick Steps.
- Cross-module: the same categories color calendar/contacts/tasks — used
  as project/context tags.
- Search: `category:"Name"`; built-in "Categorized mail" search folder;
  group-by-category in classic.
- Flat namespace, no nesting; stripped from outbound mail by default.
- **TUI mapping**: categories map naturally to notmuch-style tags but **do
  not traverse IMAP** (§9) — a Graph-backed client gets them for free; an
  IMAP-backed client can only approximate with local tags.

## 9. Protocol Access — The Critical Section

### 9.1 IMAP/SMTP status

- IMAP/POP/SMTP still enabled by default, but **Basic auth is dead**
  (M365 IMAP/POP since Oct 2022; SMTP AUTH Basic enforcement completes
  April 30, 2026; Outlook.com killed Basic in Sept 2024).
- **OAuth 2.0 `AUTH=XOAUTH2` is mandatory** (OAUTHBEARER not offered) for
  `outlook.office365.com:993` / `smtp.office365.com:587`.
- OAuth requirements: Entra ID **app registration**; public client with
  Auth Code + PKCE or **Device Code flow** (ideal for TUIs); scopes
  `IMAP.AccessAsUser.All`, `SMTP.Send`, `offline_access`; refresh tokens
  expire if unused ~90 days.
- **Tenant policy can disable IMAP per mailbox/tenant**, and admins can
  require consent for any third-party app — expect "AADSTS65001 consent
  required" failures and document the admin-approval path.

### 9.2 IMAP server quirks (Exchange Online)

- **Thin capability set**: IMAP4rev1, IDLE, MOVE, UIDPLUS, ID, UNSELECT,
  CHILDREN, NAMESPACE, AUTH=XOAUTH2. **No CONDSTORE/QRESYNC** (no cheap
  flag resync), **no THREAD/SORT** (thread locally), **no SPECIAL-USE** —
  match well-known folders by name.
- **Folder names are localized** ("Sent Items"/"Elementos enviados") and
  depend on the first client's locale; mixed-client use can create
  duplicate system folders. Delimiter `/`.
- **No custom keywords persisted** — only system flags. Consequence:
  **categories, pins, snooze, Focused/Other, flag due-dates are all
  invisible over IMAP**; \Flagged is a binary follow-up flag.
- Not exposed over IMAP: Search Folders, Recoverable Items, Online
  Archive mailbox.
- Threading: Outlook's conversation grouping ≠ References threads — a
  References-threading TUI will split/merge differently than users saw.
- Classics: TNEF `winmail.dat` from classic-Outlook senders; SMTP
  submission does **not** auto-file to Sent Items (client must APPEND —
  opposite of Gmail; get it wrong and you have duplicates or nothing);
  IDLE pushes only the selected folder; ~16–20 connection throttle;
  30-minute idle session limit.

### 9.3 Microsoft Graph — the modern path

- REST/JSON, OAuth-only; works for both M365 and personal Outlook.com.
- What Graph gives that IMAP cannot: `categories` + master list,
  `inferenceClassification` (Focused/Other) + overrides, `flag` with
  dates, `messageRules` CRUD, **`mailSearchFolder` CRUD (search
  folders!)**, `$search` (KQL), conversationId, **delta queries** for
  efficient sync, locale-independent well-known folder names, large-file
  upload sessions, correct sent-items handling, event RSVP endpoints, raw
  MIME in/out (`$value`).
- Still not exposed: pin, snooze, Sweep rules, Quick Steps.
- Hurdles: shipping an app registration; many orgs require admin consent
  for `Mail.ReadWrite`/`Mail.Send`; throttling (429 + Retry-After handling
  mandatory); no push without a public HTTPS webhook — desktop clients
  poll delta (cheap).
- **EWS is a dead end**: third-party requests blocked from Oct 1, 2026,
  fully disabled April 2027. Do not build on EWS.
- **Implication for nitidus**: IMAP+XOAUTH2 is the baseline backend; a
  native **Graph backend** is the only way to get categories, Focused
  Inbox, search folders, and rules — a strong post-MVP candidate.

### 9.4 Exchange ActiveSync

Phone-sync protocol; proprietary, limited, being displaced by Graph.
Irrelevant for a TUI — not specced.

## 10. What Outlook Users Miss in IMAP Clients

1. **Categories** — color tags vanish; the #1 organizational loss.
2. **Focused Inbox** — one undifferentiated stream.
3. **Search Folders / saved searches** — invisible; per-folder client
   search is slower.
4. **Calendar integration** — invites as raw .ics/opaque attachments, no
   inline RSVP, invite auto-deletion confuses.
5. **Pins and snooze** — gone; snoozed mail "reappears from nowhere."
6. **Sweep and rules editing** — rules run but can't be managed.
7. **Server-grade whole-mailbox search** with operators.
8. **Conversation grouping differences** vs what Outlook showed.
9. **Flag due dates → To Do** reduced to binary \Flagged.
10. Misc: winmail.dat blobs, localized/duplicate folders, no online
    archive, encrypted (OME) mail unreadable, no OOF setting, no GAL.

## 11. The Best Parts — What Nitidus Should Imitate

- **Search folders as first-class virtual folders** (persistent,
  always-updating, in the sidebar) — biggest familiarity + power win;
  with a Graph backend, sync the server's search folders.
- **Categories as colored multi-tags** with a master list, `category:`
  search, and a categorize key — native via Graph; local tag store on
  IMAP.
- **Sweep** — "delete all from sender / keep latest / always delete
  future" as one command; trivially a rule + bulk-op in a TUI.
- **Quick Steps** — named multi-action macros bound to keys (a TUI is
  their natural home; nitidus's command-chain macros cover this).
- **Focused Inbox** — render the Graph classification as two views +
  "always move" overrides; optionally a local classifier for IMAP.
- **Undo (`Z`)** for archive/delete/move; **`E`** one-key archive;
  **selectable keymap schemes** (nitidus: mutt-style and Gmail-style).
- **Snooze** (local hide + resurface is fine).
- **Conditional formatting** of the index by rule — Outlook refugees lost
  this even in new Outlook; nitidus's pattern-driven index colors deliver
  it.
- **Inline iTIP RSVP** with graceful invite rendering.
- Compose niceties: attachment reminder, scheduled send, undo-send delay,
  @mention highlighting, multiple signatures with new-vs-reply defaults,
  template snippets, external-recipient warning.
- **Rules editor** for server-side rules (via Graph) so users never need
  OWA.

## Sources

- Microsoft Learn: Deprecation of EWS in Exchange Online; Deprecation of
  Basic authentication; POP3 and IMAP4 in Exchange Online; Outlook mail
  API overview (Graph); inferenceClassification / Focused Inbox; message
  resource; keyboard shortcuts for Outlook
- Microsoft Support: Flag or pin a message; known Basic-auth issues
- Community/ecosystem: Thunderbird Microsoft OAuth notes (Mozilla),
  EighTwOne Exchange IMAP OAuth2 guide, M365 IMAP capability gists,
  modern-auth enforcement timelines
