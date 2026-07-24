---
name: contrib
description: Creates a new design doc based on rules/contributing.md.
---

# Contrib: start a design doc

Creates a new contribution document in `design/` following the templates in
`rules/contributing.md`. This skill produces a _document_, not code — never
start implementing until after the discussion section has been finished in 1 or
more rounds of Q/A sessions.

## 1. Classify and name it

Pick the type from what the user described:

- **feature** — new behavior or content.
- **refactor** — restructuring with behavior unchanged; any intentional behavior
  change must be called out in the proposal or it's a feature.
- **chore** — mechanical maintenance with no design decisions; if a real design
  question surfaces, promote it.
- **bugfix** — behavior deviates from the intended design; if fixing it means
  rethinking the design itself, promote it to a feature or refactor.

File name is `{type}-{description}-v{n}.md` with a 1-2 word dash-separated
description (e.g. `feature-example-v1.md`). Check `design/` for existing docs on
the same topic: a fresh take on a shipped or abandoned doc bumps the version;
otherwise start at `v1`. If the type or scope is genuinely ambiguous, ask before
creating the file.

## 2. Research before writing

The pre-discussion section must describe what actually exists, not what you
assume. Read the relevant code, documentation, or design docs first.

## 3. Fill in the template

Use the matching template from `rules/contributing.md` verbatim in structure. On
creation, fill only the sections you can honestly fill, if you need to ask
questions from the user before filling out the pre-discussion sections, ask in
chat instead of asking in the document.

## 4. Stop and hand off

After writing the doc, summarize the proposal in a few sentences, point the user
at the q1 questions, and stop. The user answers in the doc (or in chat, in which
case you transcribe the conversation into the discussion section) — only then
does the post discussion sections get filled out.
