# Contributing via Claude

New features should always be documented in the design folder in project root.

Feature markdown files should be named {type}-{description}-v{n}.md, where type
is feature|refactor|chore|bugfix, description is a 1-2 word description using
dash separators, and the version number like v1, v2.

When a design document is started, a new git branch should be created, using the
same name as the design document, with the initial document as the first commit,
with the the pre-discussion sections filled out, and all bracketed tags removed.

Before implementing, the design document should have all implementation and
pre-implementation steps finished and committed.

After implementation, the design document should be updated with all
post-implementation steps completed, and commited with the actual code changes.

After final commit, the document and code changes should be reviewed by a human,
and then after approval and confirmation, the branch should be squashed merged
into the main branch, with a concise summary of what was changed.

## Templates

Use the following templates when creating a new contribution document in the
design folder.

### Feature

```md
# {type} - {human-readable-desc} - {version}

{overview and purpose}

## 1. Current Design

{outline current design if applicable}

## 2. Proposal

{proposed change}

## 3. Discussion

{discussion rounds, headed as [### 3.1 R1 Questions ... ### 3.2 R1 Answers]{n},
ad nauseum, with questions from claude, and answers from the user}

## 4. Plan

{phased implementation plan; each phase should leave the workspace compiling and
tests green}

## 5. Verification

{how behavior-preservation was proven: clippy, full test run with pass counts,
any before/after comparisons}

## 6. Implementation Report

{report from claude on implementation details, success, issues, and follow-up
items to be done later.}

## 7. Testing and Cleanup

{report on test runs and cleanup after running verify and cleanup skills}
```

### Refactor

A refactor restructures code without changing behavior. Any intentional behavior
change must be called out explicitly in the proposal, or it belongs in a feature
doc instead.

```md
# {type} - {human-readable-desc} - {version}

{overview: what is being restructured and why now}

## 1. Current Design

{what exists today: the modules/files involved and the problems driving the
refactor}

## 2. Proposal

{target structure; state that behavior is unchanged, or list any intentional
exceptions}

## 3. Discussion

{discussion rounds, headed as [### 3.1 R1 Questions ... ### 3.2 R1 Answers]{n},
ad nauseum, with questions from claude, and answers from the user}

## 4. Plan

{phased implementation plan; each phase should leave the workspace compiling and
tests green}

## 5. Verification

{how behavior-preservation was proven: clippy, full test run with pass counts,
any before/after comparisons}

## 6. Implementation Report

{report from claude on implementation details, success, issues, and follow-up
items to be done later.}

## 7. Testing and Cleanup

{report on test runs and cleanup after running verify and cleanup skills}
```

### Bugfix

A bugfix corrects behavior that deviates from the intended design. The doc
records the symptom, the root cause, and the fix. If the "fix" requires
rethinking the intended design itself, it belongs in a feature or refactor doc
instead.

```md
# {type} - {human-readable-desc} - {version}

{one-line summary of the bug}

## 1. Symptoms and Causes

{observed wrong behavior; how to reproduce it (seed, commands, save state)}

## 2. Proposed Fix

{the proposed change; call out any behavior beyond the bug that it touches}

## 3. Discussion

{discussion rounds, headed as [### 3.1 R1 Questions ... ### 3.2 R1 Answers]{n},
ad nauseum, with questions from claude, and answers from the user}

## 4. Plan

{the steps, including a regression test that fails before the fix and passes
after}

## 5. Verification

{commands run and their results: the regression test red-then-green, clippy,
full test run with pass counts}

## 6. Implementation Report

{report from claude on what was done, anything surprising, and follow-up items.}

## 7. Testing and Cleanup

{report on test runs and cleanup after running verify and cleanup skills}
```

### Chore

A chore is mechanical maintenance (dependency bumps, tooling, cleanup, doc
moves) with no design decisions. If a chore surfaces a real design question,
stop and promote it to a feature or refactor doc.

```md
# {type} - {human-readable-desc} {version}

{what the chore is and why it's needed}

## 1. Scope

{files, crates, or tooling touched; what is explicitly out of scope}

## 2. Discussion

{discussion rounds, headed as [### 3.1 R1 Questions ... ### 3.2 R1 Answers]{n},
ad nauseum, with questions from claude, and answers from the user}

## 3. Plan

{the steps, usually a single pass}

## 4. Verification

{commands run and their results: build, clippy, tests with pass counts}

## 5. Implementation Report

{report from claude on what was done, anything surprising, and follow-up items.}

## 6. Testing and Cleanup

{report on test runs and cleanup after running verify and cleanup skills}
```
