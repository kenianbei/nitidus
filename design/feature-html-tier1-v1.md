# feature - HTML Tier 1 - v1

Roadmap item 1b.11. Replace the pager's raw-HTML fallback with sanitized, styled
native rendering: ammonia strips scripts, trackers, and remote content;
html2text's rich API converts the surviving DOM into width-wrapped lines of
annotated spans that map onto theme styles. Anchor hrefs feed the link picker
directly instead of being regex-scraped from rendered text.

Tiers 2 (inline images via terminal graphics) and 3 (headless-Chromium pixel
rendering) are Phase 4 and out of scope here.

## 1. Current Design

The pager (feature-pager-v1, on the branch this one builds on) fetches raw
message bytes, parses them into a pure `MessageView` (`nitidus-mail::message`),
and renders the selected part:

- `MessageView::parse` flattens leaf parts into `PartView`s with
  `PartKind:: {Text, Html, Other}`; `text` holds mail-parser's decoded text for
  both text and HTML parts — for HTML that is the raw markup.
- `default_part` prefers the first text/plain body part, falling back to HTML.
- `pager/body.rs` is the pure body pipeline for plain text: format=flowed
  reflow, `quote_depth` classification into
  `LineKind::{Normal, Quote(n), Signature}`, width wrapping with quote prefixes,
  `extract_links` (regex-ish scan for `http(s)://` runs), `skip_quoted_target`.
- `pager/render.rs::build_message_lines` renders an HTML part by pushing a
  warning line — `[text/html shown raw — styled rendering lands with tier 1]` —
  and then feeding the raw markup through the plain-text pipeline. Real Gmail
  mail is mostly HTML-only, so most messages currently display as tag soup
  behind that banner.
- The link picker (`pager/ops.rs::links`) runs `extract_links` over an unwrapped
  rebuild of the current part — on HTML parts it scans raw markup, matching URLs
  inside attribute values by accident.
- Scroll and skip-quoted operate on the window's parallel `Vec<LineKind>`.

`rust-libraries.md` §8 already assessed the tier-1 crates: **html2text 0.17.1**
(`config::rich()` → `TaggedLine`s of spans tagged with `RichAnnotation`;
optional `css` feature) and **ammonia 4.1.4** (html5ever sanitizer;
`attribute_filter` callback for remote-content stripping; `url_schemes`
allowlist).

## 2. Proposal

A new pure module `crates/nitidus/src/pager/html.rs` and a small render-side
mapping; `nitidus-mail` stays free of rendering dependencies.

### 2.1 Sanitize

`sanitize(html) -> Sanitized { html: String, blocked_remote: usize }` using
ammonia:

- Default tag/attribute allowlist (scripts, event handlers, iframes, forms
  already stripped with contents).
- `attribute_filter` drops `src`/`srcset`/`poster`/`background` values with
  `http:`/`https:` schemes (counting them as blocked remote content) while
  letting `cid:` and `data:` image references through untouched for tier 2 to
  use later. Anchor `href`s are _not_ filtered — links are surfaced, never
  fetched.
- No load-remote toggle in this item; the count is surfaced so a future
  config/keybinding item has something to flip.

### 2.2 Render

`render_html(sanitized, width) -> Vec<HtmlLine>` where `HtmlLine` is a list of
`(String, SpanStyleTag)` spans plus the line's `LineKind` (so the existing
scroll/skip-quoted machinery keeps working unchanged):

- html2text `config::rich()` at the given width produces `TaggedLine`s; each tag
  set of `RichAnnotation`s collapses into a small owned `SpanStyleTag`
  bitset-style struct (link, emphasis, strong, strikeout, code/preformat,
  header) so the pure module exposes no html2text types.
- Blockquotes arrive as `> `-prefixed text; `body::quote_depth` classifies them,
  so quote coloring and `S` (skip-quoted) work identically on HTML and plain
  parts.
- Images render as their alt-text placeholder (html2text default); tier 2
  replaces these with real graphics.
- The `css` feature is left off: ammonia strips `<style>` blocks and `style=`
  attributes before html2text would see them, so the feature could only ever see
  nothing. CSS-driven colors are revisited in tier 2/3.

### 2.3 Anchors into the link picker

`render_html` also returns the document's anchors in order:
`Vec<Anchor { href, label }>`, deduped, `http:`/`https:`/`mailto:` only. For
HTML parts the link picker lists these (label as picker label, href as detail
and open target) instead of regex-scanning; plain-text parts keep
`extract_links` unchanged. `mailto:` entries open via the same `xdg-open` path.

### 2.4 Wiring

- `render.rs::build_message_lines`: for `PartKind::Html`, replace the warning
  banner + raw text with the sanitize→render pipeline; map each `SpanStyleTag`
  to theme styles (link → info + underline, strong → bold, emphasis → italic,
  code/preformat → disabled/dim, header → bold, strikeout → crossed-out). When
  `blocked_remote > 0`, one info line at the top: `[N remote images blocked]`.
- `default_part` preference (text/plain first) is unchanged; `]`/`[` still
  switch parts.
- Pipeline cost (sanitize + render per window rebuild) is accepted for tier 1:
  rebuilds happen on open/part/header toggles, not on scroll or cursor movement.
  If profiling shows large newsletters lag, caching keyed on (id, part, width)
  is the follow-up, not a redesign.
- New workspace dependencies: `ammonia = "4.1"`, `html2text = "0.17"`.

## 3. Discussion

### 3.1 R1 Questions

1. **Part preference.** `default_part` currently prefers text/plain, falling
   back to HTML (the mutt convention — the plain part is the sender-authored
   fallback). With styled HTML now looking decent, marketing mail's plain parts
   are often worse than its HTML. Keep text/plain-first (my recommendation for
   tier 1, matching the pager doc's decision), or flip to HTML-first?
2. **Remote-content notice.** One dim/info line at the top of the body —
   `[3 remote images blocked]` — only when something was actually stripped.
   Sufficient, or would you rather it live in the statusline part segment?
3. **CSS feature off.** Since ammonia removes `<style>`/`style=` before
   html2text runs, I propose building html2text without its `css` feature and
   deferring CSS-driven color to the later tiers. Confirm?
4. **Anchor scheme allowlist.** Link picker for HTML parts lists `http:`,
   `https:`, and `mailto:` anchors (deduped, document order), opening all via
   `xdg-open`. Anything else worth keeping (e.g. `tel:`), or is that set right?
5. **Style mapping.** Proposed: link → info + underline, strong → bold, emphasis
   → italic, code/preformat → disabled style, header → bold, strikeout →
   crossed-out, blockquote → existing quote-depth colors. Any adjustments?

### 3.2 R1 Answers

1. agreed, but is there a way to switch back and forth?
2. info line is fine
3. confirmed, make a note somewhere
4. that's all good.
5. looks good.

### 3.3 R2 Notes

1. Switching already exists: `]` / `[` cycle the body parts (`switch_part`),
   and the statusline part segment shows e.g. `text/html 2/2` while off the
   default part. Plain-first therefore costs one keypress per message to see
   the styled HTML; no new work needed.
2. The css-feature deferral gets a durable note in
   `documentation/specification.md` (its html2text entry currently lists the
   `css` feature as part of tier 1) and in the `html.rs` module docs.

## 4. Plan

Each phase leaves the workspace compiling, clippy-clean, and tests green.

**Phase 1 — pure HTML pipeline, wired.** Add `ammonia = "4.1"` and
`html2text = "0.17"` (no `css` feature) to the workspace and `nitidus` crate.
New `crates/nitidus/src/pager/html.rs`:

- `sanitize(html) -> Sanitized { html, blocked_remote }` — ammonia defaults
  plus an `attribute_filter` dropping `http(s):` `src`/`srcset`/`poster`/
  `background` values (counted), keeping `cid:`/`data:`; anchor `href`s
  untouched.
- `render_html(html, width) -> RenderedHtml { lines, anchors }` — html2text
  rich mode; each line becomes `HtmlLine { spans: Vec<(String, SpanStyleTag)>,
  kind: LineKind }` with `kind` from `body::quote_depth` on the line text;
  `SpanStyleTag { link, strong, emphasis, strikeout, code, header }`;
  `anchors: Vec<Anchor { href, label }>` deduped, `http`/`https`/`mailto`
  only, document order.

Wire into `render.rs::build_message_lines`: the `PartKind::Html` branch
replaces the raw-text fallback + banner with sanitize→render, a
`SpanStyleTag`→theme style mapping (per §2.4), and the
`[N remote images blocked]` info line when `blocked_remote > 0`. Unit tests
in `html.rs`: script stripped, remote img blocked and counted, `cid:` kept,
anchors deduped/filtered/labeled, blockquote lines classified as quotes,
strong/emphasis spans tagged, width respected.

**Phase 2 — anchors into the link picker.** `ops.rs::links`: for HTML parts,
build the picker from `render_html` anchors (label as picker label, href as
detail and open target); plain-text parts keep `extract_links`. Integration
coverage in `crates/nitidus/tests/pager.rs`: opening an HTML message renders
styled lines without the banner, and the link op lists anchor hrefs.

**Phase 3 — docs + smoke.** Update `documentation/specification.md`'s
html2text entry (css feature deferred to tier 2/3 with the ammonia
rationale). Pty smoke against the kenianbei corpus: open an HTML-only
message, confirm styled rendering, blocked-images line, quote coloring, and
the anchor link picker. Record results in §5/§6.

## 5. Verification

- `cargo clippy --workspace --all-targets` — zero warnings.
- `CARGO_INCREMENTAL=0 cargo test --workspace` — **182 passed, 0 failed**
  (was 170 before this feature: +11 `html.rs` unit tests, +1 pager
  integration test; the existing links test gained item-level assertions).
- Pty smoke against the live kenianbei corpus (pyte replay, text + per-cell
  styles), on a real Gmail security-alert message:
  - HTML part renders clean styled text — no banner, no tag soup; `<hr>`
    rules and paragraphs intact.
  - `[3 remote images blocked]` info line present, info-colored.
  - Links (`remove`, `Check activity`, the bare notifications URL) render
    info-blue + underlined; header names bold.
  - `l` opens the links picker with three anchors: labels (`remove`,
    `Check activity`) with href details, the bare-URL anchor falling back
    to its href; the tracker pixel absent.

## 6. Implementation Report

Implemented as designed, with these deviations and notes:

- **No `is_header` tag.** `RichAnnotation` has no header variant; html2text
  renders `h1..h6` as `# `-prefixed text lines instead. Headers therefore
  arrive markdown-style rather than bold — acceptable for tier 1.
- **`RichAnnotation` is `#[non_exhaustive]`**, so `collapse_annotations`
  carries a wildcard arm (images and css-only color annotations render
  unstyled).
- **`ActiveOverlay::visible_items()`** was added so integration tests can
  assert picker content through the public API (previously only `is_open`).
- **`pager/ops.rs` split.** Adding `current_anchors` pushed the file past the
  300-line limit (it was already at 309 pre-feature), so attachment
  persistence and the system-opener moved to a new `pager/save.rs`;
  `save_attachment`/`open_attachment` now share a `write_unique` helper.
  Behavior unchanged, covered by the existing save/open tests.
- Anchors for the link picker render at a fixed width
  (`ANCHOR_RENDER_WIDTH = 200`); wrapping is irrelevant because consecutive
  same-href spans merge.
- `documentation/specification.md`'s html2text entry now records the
  css-feature deferral (R2 note 2).

Follow-ups for later items: a load-remote toggle (config/keybinding) fed by
the `blocked_remote` count; `cid:`/`data:` image references pass sanitization
untouched, ready for tier 2 inline rendering.

Post-implementation observation: `cargo fmt` would reformat several files
untouched by this feature (thread.rs, index/*, tests) — left for a separate
formatting chore so this diff stays scoped.

## 7. Testing and Cleanup
