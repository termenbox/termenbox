#!/bin/sh
# Tests for scripts/pool.sh against a throwaway origin. Run: sh scripts/pool.test.sh
set -u
here=$(cd "$(dirname "$0")" && pwd)
pool="$here/pool.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# Nothing below may touch the repository this runs in: work from the temp directory, and check every path.
cd "$tmp" || exit 1
inside() { case $1 in "$tmp"/?*) ;; *) echo "refusing to touch '$1': outside $tmp" >&2; exit 2 ;; esac; }
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
export POOL_MAX=4 POOL_WARM=2 POOL_NO_BG=1
fails=0
ok() { printf 'ok   %s\n' "$1"; }
bad() { printf 'FAIL %s\n' "$1"; fails=$((fails + 1)); }
check() { d=$1; shift; if "$@" >/dev/null 2>&1; then ok "$d"; else bad "$d"; fi; }
refuse() { d=$1; shift; if "$@" >/dev/null 2>&1; then bad "$d"; else ok "$d"; fi; }
eq() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1 (got '$2', want '$3')"; fi; }

n=0
setup() {
  n=$((n + 1))
  base="$tmp/s$n"
  git init -q --bare -b develop "$base/origin.git"
  git clone -q "$base/origin.git" "$base/repo" 2>/dev/null
  (cd "$base/repo" && git switch -q -c develop && echo a >a && git add a && git commit -qm init && git push -q origin develop)
  repo="$base/repo"
  wt="$base/repo-wt"
}
pool() { (cd "$repo" && sh "$pool" "$@"); }
slots() { ls -d "$wt"/pool-* 2>/dev/null | wc -l | tr -d ' '; }
branch_of() { git -C "$1" symbolic-ref -q --short HEAD || echo detached; }

# --- warm keeps WARM free slots ---
setup
check "warm succeeds" pool warm
eq "warm creates the warm slots" "$(slots)" 2
eq "a warm slot is detached" "$(branch_of "$wt/pool-01")" detached

# --- acquire ---
p=$(pool acquire fix/one)
inside "$p"
eq "acquire prints the slot path" "$p" "$wt/pool-01"
eq "acquire checks out the branch" "$(branch_of "$p")" fix/one
eq "acquire starts from origin/develop" "$(git -C "$p" merge-base HEAD origin/develop)" "$(git -C "$repo" rev-parse origin/develop)"
eq "acquire again is idempotent" "$(pool acquire fix/one)" "$p"
eq "acquire tops the pool back up to the warm count" "$(slots)" 3
for bad_name in main feature/x 'fix/Bad Name' fix/ develop; do
  refuse "acquire refuses '$bad_name'" pool acquire "$bad_name"
done

# --- the pool is limited ---
pool acquire fix/two >/dev/null
pool acquire fix/three >/dev/null
pool acquire fix/four >/dev/null
eq "the pool never holds more than the maximum" "$(slots)" 4
refuse "a full pool refuses another task" pool acquire fix/five
pool acquire fix/five 2>&1 | grep -qi exhausted && ok "a full pool says why" || bad "a full pool says why"

# --- release ---
echo x >"$p/dirty"
refuse "release refuses uncommitted work" pool release "$p"
rm "$p/dirty"
(cd "$p" && echo y >y && git add y && git commit -qm work)
refuse "release refuses commits that exist nowhere else" pool release "$p"
git -C "$p" push -q origin HEAD:refs/heads/fix/one
check "release works once the work is pushed" pool release "$p"
eq "a released slot is detached again" "$(branch_of "$p")" detached
refuse "a released slot has no claim, acquire hands it out again" test "$(pool acquire fix/five)" != "$p"
echo z >"$p/dirty"
check "release --force drops uncommitted work" pool release --force "$p"

# --- reap gives back merged slots by itself ---
setup
pool acquire fix/m1 >/dev/null
pool acquire fix/m2 >/dev/null
pool acquire fix/m3 >/dev/null
m1=$wt/pool-01 m2=$wt/pool-02 m3=$wt/pool-03
inside "$m1"
(cd "$m1" && echo 1 >f1 && git add f1 && git commit -qm w1 && git push -q -u origin HEAD:refs/heads/fix/m1)
(cd "$m2" && echo 2 >f2 && git add f2 && git commit -qm w2 && git push -q -u origin HEAD:refs/heads/fix/m2)
(cd "$m3" && echo 3 >f3 && git add f3 && git commit -qm w3 && git push -q -u origin HEAD:refs/heads/fix/m3)
pool reap >/dev/null 2>&1
eq "reap keeps a slot whose branch is not merged" "$(branch_of "$m1")" fix/m1
# squash merge of m1 into develop, then its remote branch is deleted
(cd "$repo" && git fetch -q origin && git switch -q develop && git merge -q --squash origin/fix/m1 && git commit -qm "squash m1" && git push -q origin develop && git push -q origin --delete fix/m1)
echo dirty >"$m3/dirty"
git -C "$repo" push -q origin --delete fix/m3
pool reap >/dev/null 2>&1
eq "reap releases a squash-merged slot (remote branch gone)" "$(branch_of "$m1")" detached
eq "reap keeps an unmerged slot" "$(branch_of "$m2")" fix/m2
eq "reap never drops uncommitted work" "$(branch_of "$m3")" fix/m3
refuse "reap deleted the merged local branch" git -C "$repo" show-ref -q --verify refs/heads/fix/m1
(cd "$repo" && git fetch -q origin && git merge -q origin/fix/m2 -m m2 && git push -q origin develop)
pool acquire fix/next >/dev/null 2>&1
eq "acquire reaps a branch contained in develop" "$(branch_of "$m2")" detached

# --- free slots beyond the warm count are removed ---
setup
pool warm >/dev/null
pool acquire fix/a >/dev/null
pool acquire fix/b >/dev/null
inside "$wt/pool-01"
git -C "$wt/pool-01" push -q origin HEAD:refs/heads/fix/a
git -C "$wt/pool-02" push -q origin HEAD:refs/heads/fix/b
pool release "$wt/pool-01" >/dev/null
pool release "$wt/pool-02" >/dev/null
free=0
for d in "$wt"/pool-*; do [ "$(branch_of "$d")" = detached ] && free=$((free + 1)); done
eq "only the warm count of free slots is kept" "$free" 2

# --- a hand-made worktree in the pool directory is never touched ---
setup
mkdir -p "$wt"
git -C "$repo" worktree add -q -b chore/mine "$wt/pool-01" origin/develop
echo keep >"$wt/pool-01/mine"
pool warm >/dev/null
eq "an unmanaged slot keeps its branch" "$(branch_of "$wt/pool-01")" chore/mine
check "an unmanaged slot keeps its files" test -f "$wt/pool-01/mine"
refuse "an unmanaged slot is not handed out" test "$(pool acquire fix/x)" = "$wt/pool-01"
check "adopt makes it a managed slot" pool adopt "$wt/pool-01"
check "a managed slot can be released" pool release --force "$wt/pool-01"

# --- status ---
setup
pool warm >/dev/null
pool acquire fix/s >/dev/null
out=$(pool status)
printf '%s' "$out" | grep -q 'fix/s' && ok "status names a claimed branch" || bad "status names a claimed branch"
printf '%s' "$out" | grep -q 'free' && ok "status shows free slots" || bad "status shows free slots"

[ "$fails" -eq 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
