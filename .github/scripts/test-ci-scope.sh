#!/bin/sh
# Tests ci-scope.sh: `<expected code> <expected scripted> <paths...>`.
dir=$(dirname "$0")
fail=0
check() {
  want="code=$1 scripted=$2"; shift 2
  got=$(printf '%s\n' "$@" | sh "$dir/ci-scope.sh" | tr '\n' ' ' | sed 's/ $//')
  [ "$got" = "$want" ] || { echo "FAIL: [$*] gave '$got', wanted '$want'"; fail=1; }
}
check false false docs/plans/x.md
check false false docs/facts/a.md README.md
check true  false packages/server/src/main.rs
check true  false packages/server/src/main.rs docs/a.md packages/agent/src/lib.rs
check true  true  packages/server/Cargo.toml
check true  true  packages/server/release-keys.txt
check true  true  packages/client/src/App.svelte
check true  true  packages/server/src/main.rs packages/client/e2e/suite.mjs
check true  true  Cargo.lock
check true  true  .github/workflows/ci.yaml
check true  true  scripts/pool.sh
check true  true
[ "$fail" = 0 ] && echo "ci-scope: all passed"
exit $fail
