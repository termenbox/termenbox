# Contributing to Termenbox

Thank you for contributing! This guide covers the essential rules and workflows.

## Branch Targeting Rule

**All feature PRs must target `develop`, not `master`.**

- `develop` is the integration branch for nightly builds
- `master` is release-only: stable/beta tags are cut from `master`
- Hotfixes to `master` are back-merged to `develop`
- This is enforced at three levels:
  1. **GitHub Branch Ruleset** on `master` — only `release-managers` team + admins can create PRs to `master`
  2. **CI Guardrail** (`.github/workflows/block-master-prs.yml`) — auto-closes non-release PRs to `master`
  3. **Local pre-push hook** (`.github/hooks/pre-push`) — blocks `git push origin master` locally

> See [FACT-0053](docs/facts/0053-develop-branch-workflow.md) for full details.

## Quick Start

### Prerequisites

- Rust 1.80+ (via `rustup`)
- Node.js 20+ and pnpm 9+
- Docker (for containerized builds)

### Local Development

```bash
# Clone and enter
git clone https://github.com/your-org/termenbox.git
cd termenbox

# Install hooks (auto-activated in devcontainer/Codespaces)
git config core.hooksPath .github/hooks

# Build everything
cargo build --workspace --release
pnpm -r build

# Run tests
cargo test --workspace
pnpm -r test
```

### Devcontainer / GitHub Codespaces

Open the repo in a devcontainer or Codespaces — the `postCreateCommand` in `.devcontainer.json` auto-configures the pre-push hook.

### Making Changes

1. Create a feature branch from `develop`:
   ```bash
   git checkout develop
   git pull
   git checkout -b feat/your-feature-name
   ```

2. Make your changes, following the commit conventions below

3. Run tests locally:
   ```bash
   cargo test --workspace && pnpm -r test
   ```

4. Push and open a PR **against `develop`**:
   ```bash
   git push origin feat/your-feature-name
   # Open PR at GitHub: base = develop
   ```

## Commit Message Convention

We use **Conventional Commits** enforced by CI:

```
type(scope): summary

Why: <reason for the change>
What: <what was changed>
```

### Types

| Type | Use For |
|------|---------|
| `feat` | New feature |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `chore` | Maintenance, deps, tooling |
| `ci` | CI/CD changes |
| `test` | Test additions/changes |
| `refactor` | Code restructuring (no behavior change) |
| `perf` | Performance improvement |
| `build` | Build system changes |
| `style` | Formatting, whitespace |
| `revert` | Reverting a previous commit |

### Scope

- Optional, one word: `[a-z0-9-]+`
- Examples: `ci(devcontainer)`, `docs(contributing)`, `fix(paths)`
- Omit when a change spans multiple areas

### Examples

```bash
# Good
ci(devcontainer): add postCreateCommand to activate pre-push hook

Why: Ensure local hook runs in devcontainer/Codespaces automatically
What: Added postCreateCommand setting git hooksPath to .github/hooks

# Good
docs(contributing): document develop-branch targeting rule

Why: Contributors need to know feature PRs target develop, not master
What: Added branch rule, quick start, and commit convention sections

# Bad (multiple scopes)
fix(server,client): handle reconnection

# Bad (no Why/What body)
feat: add new feature
```

## Pre-push Hook

The hook at `.github/hooks/pre-push` blocks direct pushes to `master`:

```bash
# This will fail:
git push origin master

# This works:
git push origin feat/my-feature
```

It's auto-activated in devcontainer/Codespaces via `.devcontainer.json`. To activate manually:

```bash
git config core.hooksPath .github/hooks
```

## CI Checks

All PRs must pass:

- `cargo test --workspace` (Rust tests)
- `pnpm -r test` (Node/Svelte tests)
- `cargo build --workspace --release` (release build)
- `pnpm -r build` (client build)
- Conventional Commits subject format
- Branch name pattern

## License

By contributing, you agree that your contributions will be licensed under the MIT License (DECISION-0011).