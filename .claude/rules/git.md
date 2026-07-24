# Git Guidelines

## Git Workflow

The `main` branch should always reflect a completed and working app state. In
general all other branches are considered proposed changes to the `main` branch,
and are merged into `main` when the changes are considered finished, resulting
in a code base where all tests and the app is in a completed and working state.

## Branch Naming Conventions

- Use the format `<type>/<description>`.
- Use lowercase letters, numbers, and hyphens in the description.
- Do not use spaces, underscores, punctuation, consecutive hyphens, or trailing
  hyphens.
- Keep branch names concise and descriptive.

### Allowed Branch Types

- `feature/` — New functionality
- `refactor/` — Changes to an existing feature
- `bugfix/` — Non-critical bug fixes
- `chore/` — General change like tooling, formatting

## Commit Conventions

In general, use Conventional Commits for formatting.

https://www.conventionalcommits.org/en/v1.0.0/#specification

## Commit Rules

- Do not commit code automatically unless explicitly requested
- Ensure the code runs correctly before committing
- Code should be commited in a new branch and then merged into main.
- Don't use git workspaces.

## Commit Message Format

```
<type>(<scope>): <subject>
```

A space follows the colon. Type values:

| type     | Purpose                                    |
| -------- | ------------------------------------------ |
| feat     | New feature                                |
| fix      | Bug fix                                    |
| docs     | Documentation or comments                  |
| style    | Code formatting (no runtime impact)        |
| refactor | Refactoring (not a new feature or bug fix) |
| perf     | Performance optimization                   |
| test     | Adding tests                               |
| chore    | Build process or tooling changes           |

Use a list when there are more than two key points:

```
feat(web): implement email verification workflow

- Add email verification token generation service
- Create verification email template with dynamic links
- Add API endpoint for token validation
```
