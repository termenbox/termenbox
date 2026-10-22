# FACT-0053: develop branch workflow for nightly

**Date:** 2026-10-22
**Status:** accepted

## Summary

The `develop` branch is the integration branch for nightly builds. It is created once from `master` and maintained with the following rules:

## Branch rules

- **Created once:** `git branch develop master` (one-time).
- **Feature PRs target `develop`** (not `master`). This is a shift from current practice.
- **Stable/beta tags are cut from `master`.** Before a release, `develop` is merged/ff into `master` (or a release branch), then the tag is pushed to `master`.
- **Hotfixes to `master`** are back-merged to `develop` (or `develop` is rebased onto `master`).
- **Branch protection on `develop`:**
  - Required status checks (CI must pass).
  - CODEOWNERS for `.github/workflows/**`, `release-keys.txt`, `.github/scripts/make_manifest.py`.
  - No force pushes.

## Technical enforcement: preventing accidental PRs to `master`

To make targeting `master` with feature PRs **technically impossible** for regular contributors, the following guardrails are configured in the repository:

### 1. GitHub Branch Ruleset on `master` (primary gate)

**Settings → Rules → Rulesets → "Protect master"**

```yaml
# Ruleset configuration (managed via GitHub UI or Terraform)
target: master
rules:
  - type: pull_request_creation_restriction
    parameters:
      restrict_creation: true
      allowed_actors:
        - type: Team
          name: release-managers   # только релиз-менеджеры / админы
        - type: RepositoryRole
          role: admin
  - type: required_status_checks
    parameters:
      strict_required_status_checks_policy: true
      required_checks:
        - context: "CI / test"
        - context: "CI / build"
  - type: linear_history
  - type: non_fast_forward
```

**Effect:** Обычные контрибьюторы **не видят** возможность создать PR в `master` в UI и API. Кнопка "Compare & pull request" для `master` недоступна.

### 2. CI Guardrail Workflow (secondary gate / defense in depth)

**File:** `.github/workflows/block-master-prs.yml`

```yaml
name: Block PRs to master
on:
  pull_request_target:
    types: [opened, synchronize, reopened, ready_for_review]

permissions:
  contents: read
  pull-requests: write

jobs:
  block-non-release-prs:
    if: github.base_ref == 'master' && github.event.pull_request.draft == false
    runs-on: ubuntu-latest
    steps:
      - name: Fail - feature PRs must target develop
        run: |
          echo "❌ PR #${{ github.event.number }} targets 'master' but is not a release PR."
          echo ""
          echo "Rule: All feature PRs must target 'develop'."
          echo "Only release-managers may open PRs against 'master' (for releases/hotfixes)."
          echo ""
          echo "Action required:"
          echo "  1. Close this PR."
          echo "  2. Open a new PR against 'develop' with the same changes."
          echo ""
          echo "If this is a legitimate release/hotfix PR, ask a release-manager to reopen it."
          exit 1

      - name: Auto-close and label
        if: failure()
        uses: actions/github-script@v7
        with:
          script: |
            github.rest.issues.addLabels({
              owner: context.repo.owner,
              repo: context.repo.repo,
              issue_number: ${{ github.event.number }},
              labels: ['wrong-base-branch', 'blocked']
            })
            github.rest.pulls.update({
              owner: context.repo.owner,
              repo: context.repo.repo,
              pull_number: ${{ github.event.number }},
              state: 'closed'
            })
            github.rest.issues.createComment({
              owner: context.repo.owner,
              repo: context.repo.repo,
              issue_number: ${{ github.event.number }},
              body: '🤖 **Auto-closed:** Feature PRs must target `develop`, not `master`.\n\nPlease close this PR and open a new one against `develop`.\n\nSee: docs/facts/0053-develop-branch-workflow.md'
            })
```

**Effect:** Даже если PR в `master` создан через API/ботом/админом по ошибке — CI мгновенно фейлится, закрывает PR, вешает лейблы `wrong-base-branch` + `blocked` и пишет понятный комментарий.

### 3. Local pre-push hook (optional, developer ergonomics)

**File:** `.github/hooks/pre-push` (устанавливается через `git config core.hooksPath .github/hooks` в devcontainer / Codespaces)

```bash
#!/bin/sh
# Prevent accidental direct push to master
while read local_ref local_sha remote_ref remote_sha; do
  if [ "$remote_ref" = "refs/heads/master" ]; then
    echo "❌ Direct push to 'master' is forbidden."
    echo "   Push your feature branch and open PR against 'develop'."
    echo "   Only release-managers may push to master (via release workflow)."
    exit 1
  fi
done
exit 0
```

**Effect:** Ловит `git push origin master` локально до отправки на сервер.

---

## Nightly workflow trigger

- Schedule: `0 0 * * *` (00:00 UTC).
- Additionally: `on.workflow_run` of CI on `develop` with `types: [completed]` and `conclusion: success`. The workflow uses the CI run's SHA (`github.event.workflow_run.head_sha`) to ensure it builds from a green commit.
- If schedule fires but no green CI run exists since last nightly, the workflow exits early (no empty nightly).

## Nightly versioning

- Git tag: `vYYYY.MM.DD-nightly` (UTC date at workflow start).
- Manifest version: `{core}-nightly.YYYYMMDD` where `core` is the Cargo version from `packages/server/Cargo.toml` (e.g., `0.1.36-nightly.20261022`).
- If Cargo version on `develop` is `0.2.0` (pre-release prep), nightly becomes `0.2.0-nightly.YYYYMMDD`.

## Consequences

- Nightly always builds from a CI-verified commit.
- `develop` receives features continuously; `master` receives releases.
- Hotfixes on `master` must be back-merged to `develop` to avoid nightly regressions.
- This is a lightweight GitFlow variant; simpler than full GitFlow because release branches are not used (tags on `master` suffice).
- **Accidental PRs to `master` are blocked at three levels:** Ruleset (UI/API), CI (workflow), local hook (push).

## Related

- DECISION-0125 (release channels)
- R70 (implementation plan)
- `.github/workflows/release-nightly.yaml`, `.github/workflows/ci.yaml`
- `.github/workflows/block-master-prs.yml` (guardrail workflow)
- `.github/hooks/pre-push` (local hook)

---

## Migration checklist (one-time)

- [ ] Create `release-managers` team in GitHub org (add current maintainers)
- [ ] Create Branch Ruleset on `master` with PR creation restriction → `release-managers` + admins
- [ ] Add `.github/workflows/block-master-prs.yml` to repo
- [ ] Add `.github/hooks/pre-push` and configure in devcontainer/Codespaces (`git config core.hooksPath .github/hooks`)
- [ ] Update CONTRIBUTING.md with "All PRs → `develop`" rule
- [ ] Announce to team: "New rule: feature PRs target `develop`. Master is release-only."