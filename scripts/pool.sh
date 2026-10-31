#!/bin/sh
# The worktree pool: all work happens in a slot of this pool, never in the main checkout (decision 0150).
#
#   scripts/pool.sh acquire <type>/<slug>   claim a slot on a new branch from origin/develop; prints its path
#   scripts/pool.sh release [--force] <slot|path>   give a slot back (refuses unsaved work unless --force)
#   scripts/pool.sh warm                    reap merged slots, refresh free ones; keep POOL_WARM of them, create or remove to match
#   scripts/pool.sh reap                    release every clean slot whose branch is merged (acquire and warm do this too)
#   scripts/pool.sh adopt <slot|path>       claim a worktree made by hand in the pool directory
#   scripts/pool.sh status                  one line per slot
#
# Slots are <main checkout>-wt/pool-01 .. pool-<POOL_MAX>. A slot is cold (no directory), free (a warm, detached
# checkout of origin/develop) or claimed (a branch, recorded in the git dir). POOL_MAX (10) caps the slots;
# POOL_WARM (3) is how many free ones are kept ready. POOL_WARM_CMD runs in a slot after it is created or refreshed
# (for example to install dependencies). POOL_NO_BG=1 makes the top-up after acquire and release synchronous.
set -eu

MAX=${POOL_MAX:-10}
WARM=${POOL_WARM:-3}
BASE=${POOL_BASE:-develop}
BRANCH_RE='^(feat|fix|docs|chore|ci|test|refactor|perf|build|style|revert)/[a-z0-9][a-z0-9._-]*$'

die() { echo "pool: $*" >&2; exit "${2:-1}"; }

common=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || die "not inside the repository"
main=${common%/.git}
[ "$main" != "$common" ] || die "cannot find the main checkout from $common"
root="$main-wt"
state="$common/termenbox-pool"
mkdir -p "$state" "$root"

slot_dir() { printf '%s/pool-%02d' "$root" "$1"; }
exists() { [ -e "$1/.git" ]; }
claim_file() { printf '%s/pool-%02d' "$state" "$1"; }
claimed() { [ -f "$(claim_file "$1")" ]; }
detached() { ! git -C "$1" symbolic-ref -q HEAD >/dev/null 2>&1; }
clean() { [ -z "$(git -C "$1" status --porcelain 2>/dev/null)" ]; }
# free: a directory that is nobody's and holds nothing but a detached checkout
is_free() { exists "$1" && ! claimed "$2" && detached "$1" && clean "$1"; }

lock() { exec 9>"$state/lock"; flock 9; }

fetch() { git -C "$main" fetch -q --prune origin 2>/dev/null || echo "pool: could not fetch origin/$BASE; using what is known" >&2; }

warm_cmd() { [ -z "${POOL_WARM_CMD:-}" ] || (cd "$1" && sh -c "$POOL_WARM_CMD") >/dev/null 2>&1 || true; }

make_slot() {
  git -C "$main" worktree add -q --detach "$(slot_dir "$1")" "origin/$BASE"
  warm_cmd "$(slot_dir "$1")"
}

refresh_slot() {
  git -C "$1" switch -q --detach "origin/$BASE" 2>/dev/null && warm_cmd "$1" || true
}

# the slot number for an argument: a number, pool-NN, or a path inside the slot
slot_of() {
  local b
  case "$1" in
    *[!0-9]*) ;;
    *) expr "$1" + 0; return ;;
  esac
  b=$(basename "$1")
  case "$b" in pool-[0-9][0-9]) expr "${b#pool-}" + 0; return ;; esac
  die "not a slot: $1"
}

free_count() {
  local c=0 n=1
  while [ "$n" -le "$MAX" ]; do is_free "$(slot_dir "$n")" "$n" && c=$((c + 1)); n=$((n + 1)); done
  echo "$c"
}
total_count() {
  local c=0 n=1
  while [ "$n" -le "$MAX" ]; do exists "$(slot_dir "$n")" && c=$((c + 1)); n=$((n + 1)); done
  echo "$c"
}

# a branch is merged when its remote branch was deleted after the merge (a squash merge leaves no ancestor), it is
# contained in origin/BASE, or its pull request is merged
merged() {
  local base
  [ "$(git -C "$main" for-each-ref --format='%(upstream:track)' "refs/heads/$1")" = "[gone]" ] && return 0
  # contained in origin/BASE counts only once the branch has moved off the commit it was created at
  base=$(sed -n 3p "$(claim_file "$2")")
  [ -n "$base" ] && [ "$(git -C "$main" rev-parse "refs/heads/$1" 2>/dev/null)" != "$base" ] \
    && git -C "$main" merge-base --is-ancestor "$1" "origin/$BASE" 2>/dev/null && return 0
  command -v gh >/dev/null 2>&1 && [ -n "$(cd "$main" && gh pr list --head "$1" --state merged --json number -q '.[0].number' 2>/dev/null)" ]
}

free_slot() {
  git -C "$2" reset -q --hard
  git -C "$2" clean -fdq
  git -C "$2" switch -q --detach "origin/$BASE"
  git -C "$main" branch -D "$1" >/dev/null 2>&1 || true
  rm -f "$(claim_file "$3")"
}

# give back every clean slot whose branch is merged, so finished tasks never occupy the pool
reap() {
  local n=1 d b
  while [ "$n" -le "$MAX" ]; do
    d=$(slot_dir "$n")
    if exists "$d" && claimed "$n"; then
      b=$(sed -n 1p "$(claim_file "$n")")
      if clean "$d" && merged "$b" "$n"; then free_slot "$b" "$d" "$n"; echo "pool: released slot $n (merged $b)" >&2; fi
    fi
    n=$((n + 1))
  done
}

# keep exactly WARM free slots where the cap allows: create missing ones, drop the surplus
top_up() {
  local n=1 d
  while [ "$n" -le "$MAX" ]; do
    d=$(slot_dir "$n")
    if is_free "$d" "$n"; then refresh_slot "$d"; fi
    n=$((n + 1))
  done
  n=1
  while [ "$(free_count)" -lt "$WARM" ] && [ "$(total_count)" -lt "$MAX" ] && [ "$n" -le "$MAX" ]; do
    exists "$(slot_dir "$n")" || make_slot "$n"
    n=$((n + 1))
  done
  n=$MAX
  while [ "$(free_count)" -gt "$WARM" ] && [ "$n" -ge 1 ]; do
    d=$(slot_dir "$n")
    if is_free "$d" "$n"; then git -C "$main" worktree remove --force "$d"; fi
    n=$((n - 1))
  done
}

background_top_up() {
  if [ "${POOL_NO_BG:-}" = 1 ]; then top_up; else (sh "$0" warm >/dev/null 2>&1 &); fi
}

cmd_acquire() {
  local branch n pick d
  [ $# -eq 1 ] || die "usage: pool.sh acquire <type>/<slug>"
  branch=$1
  printf '%s' "$branch" | grep -Eq "$BRANCH_RE" || die "branch '$branch' does not match $BRANCH_RE"
  lock
  # the task already has a slot: hand it out again
  n=1
  while [ "$n" -le "$MAX" ]; do
    if claimed "$n" && [ "$(sed -n 1p "$(claim_file "$n")")" = "$branch" ]; then slot_dir "$n"; echo; return; fi
    n=$((n + 1))
  done
  fetch
  reap
  pick=
  n=1
  while [ "$n" -le "$MAX" ]; do
    if is_free "$(slot_dir "$n")" "$n"; then pick=$n; break; fi
    n=$((n + 1))
  done
  if [ -z "$pick" ]; then
    n=1
    while [ "$n" -le "$MAX" ]; do
      if ! exists "$(slot_dir "$n")"; then make_slot "$n"; pick=$n; break; fi
      n=$((n + 1))
    done
  fi
  [ -n "$pick" ] || die "pool exhausted: all $MAX slots are in use; release one (scripts/pool.sh status)" 3
  d=$(slot_dir "$pick")
  refresh_slot "$d"
  if git -C "$d" show-ref -q --verify "refs/heads/$branch"; then
    git -C "$d" switch -q "$branch"
  else
    git -C "$d" switch -q -c "$branch" "origin/$BASE"
  fi
  printf '%s\n%s\n%s\n' "$branch" "$(date +%s)" "$(git -C "$d" rev-parse HEAD)" >"$(claim_file "$pick")"
  background_top_up
  echo "$d"
}

cmd_release() {
  local force=0 n d branch
  force=0
  if [ "${1:-}" = "--force" ]; then force=1; shift; fi
  [ $# -eq 1 ] || die "usage: pool.sh release [--force] <slot|path>"
  n=$(slot_of "$1")
  d=$(slot_dir "$n")
  exists "$d" || die "slot $n does not exist"
  lock
  claimed "$n" || die "slot $n is not claimed"
  branch=$(sed -n 1p "$(claim_file "$n")")
  if [ "$force" -ne 1 ] && ! { clean "$d" && merged "$branch" "$n"; }; then
    clean "$d" || die "slot $n has uncommitted work; commit it or use --force"
    if [ -n "$(git -C "$d" rev-list "$branch" --not --remotes 2>/dev/null | head -1)" ]; then
      die "branch $branch has commits that exist nowhere else; push it or use --force"
    fi
  fi
  git -C "$d" reset -q --hard
  git -C "$d" clean -fdq
  git -C "$d" switch -q --detach "origin/$BASE"
  git -C "$main" branch -d "$branch" >/dev/null 2>&1 || true
  rm -f "$(claim_file "$n")"
  background_top_up
  echo "released slot $n (was $branch)"
}

cmd_adopt() {
  local n d
  [ $# -eq 1 ] || die "usage: pool.sh adopt <slot|path>"
  n=$(slot_of "$1")
  d=$(slot_dir "$n")
  exists "$d" || die "slot $n does not exist"
  detached "$d" && die "slot $n has no branch to claim"
  lock
  printf '%s\n%s\n' "$(git -C "$d" symbolic-ref --short HEAD)" "$(date +%s)" >"$(claim_file "$n")"
  echo "adopted slot $n"
}

cmd_warm() { lock; fetch; reap; top_up; }

cmd_reap() { lock; fetch; reap; }

cmd_status() {
  local n=1 d
  while [ "$n" -le "$MAX" ]; do
    d=$(slot_dir "$n")
    if ! exists "$d"; then
      printf 'pool-%02d  cold\n' "$n"
    elif claimed "$n"; then
      printf 'pool-%02d  claimed    %s%s\n' "$n" "$(sed -n 1p "$(claim_file "$n")")" "$(clean "$d" || echo '  (uncommitted)')"
    elif is_free "$d" "$n"; then
      printf 'pool-%02d  free\n' "$n"
    else
      printf 'pool-%02d  unmanaged  %s  (not handed out; scripts/pool.sh adopt %d)\n' "$n" "$(git -C "$d" symbolic-ref -q --short HEAD || echo detached)" "$n"
    fi
    n=$((n + 1))
  done
  echo "slots: $(total_count) of $MAX, free: $(free_count) (kept warm: $WARM)"
}

cmd=${1:-status}
[ $# -eq 0 ] || shift
case "$cmd" in
  acquire) cmd_acquire "$@" ;;
  release) cmd_release "$@" ;;
  adopt) cmd_adopt "$@" ;;
  warm) cmd_warm ;;
  reap) cmd_reap ;;
  status) cmd_status ;;
  *) die "usage: pool.sh acquire|release|adopt|warm|reap|status" ;;
esac
