#!/bin/sh
# Tests for .claude/hooks/worktree-only.py. Run: sh scripts/worktree-guard.test.sh
set -u
here=$(cd "$(dirname "$0")" && pwd)
hook="$here/../.claude/hooks/worktree-only.py"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
repo="$tmp/repo"
wt="$tmp/repo-wt/pool-01"
git init -q -b develop "$repo"
(cd "$repo" && echo a >a && git add a && git commit -qm init)
git -C "$repo" worktree add -q --detach "$wt"
fails=0
ok() { printf 'ok   %s\n' "$1"; }
bad() { printf 'FAIL %s\n' "$1"; fails=$((fails + 1)); }
# run <cwd> <tool> <field> <value>: exit status of the hook
run() { jq -n --arg cwd "$1" --arg tool "$2" --arg f "$3" --arg v "$4" '{cwd:$cwd,tool_name:$tool,tool_input:{($f):$v}}' | TERMENBOX_POOL_ONLY=${POOL_ONLY-1} python3 "$hook" >/dev/null 2>&1; }
allow() { d=$1; shift; if run "$@"; then ok "$d"; else bad "$d"; fi; }
deny() { d=$1; shift; if run "$@"; then bad "$d"; else ok "$d"; fi; }

deny  "Edit in the main checkout"            "$repo" Edit file_path "$repo/a"
deny  "Write a new file in a new main subdir" "$repo" Write file_path "$repo/new/dir/x"
deny  "NotebookEdit in the main checkout"    "$repo" NotebookEdit notebook_path "$repo/n.ipynb"
allow "Edit in a worktree"                   "$repo" Edit file_path "$wt/a"
allow "Write outside any repository"         "$repo" Write file_path "$tmp/elsewhere"
allow "Read is never blocked"                "$repo" Read file_path "$repo/a"

for c in 'ls' 'git status' 'git log --oneline | head -5' 'sh scripts/pool.sh status' 'cat a' 'grep -rn x .' 'find . -name x' 'git worktree list' 'gh pr view 1' "cd \"$wt\" && touch f" "git -C \"$wt\" commit -qm x" "cd \"$wt\"; cargo test"; do
  allow "main cwd may run: $c" "$repo" Bash command "$c"
done
for c in 'touch f' 'echo hi > f' 'cargo build' 'git commit -m x' 'git checkout -b x' 'git add .' 'rm -rf x' 'ls $(touch f)' 'ls `touch f`' 'ls; touch f' 'ls && rm f' 'sed -i s/a/b/ a' 'find . -delete' 'tee f' "git -C \"$repo\" commit -m x" "cd \"$repo\" && touch f" "cd \"$wt\" && cd \"$repo\" && touch f" 'sh scripts/dev.sh cargo test' 'cat a >> a' 'git push origin develop'; do
  deny "main cwd may not run: $c" "$repo" Bash command "$c"
done
for c in 'touch f' 'cargo build' 'git commit -m x'; do
  allow "worktree cwd may run: $c" "$wt" Bash command "$c"
done
allow "a directory that is no repository is not the main checkout" "$tmp" Bash command 'touch f'
deny  "a hook given garbage fails closed in the main checkout" "$repo" Bash command ''
POOL_ONLY=0 allow "without TERMENBOX_POOL_ONLY=1 the main checkout is not locked" "$repo" Edit file_path "$repo/a"
POOL_ONLY= allow "an unset-like empty value does not enforce" "$repo" Bash command 'touch f'

[ "$fails" -eq 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
