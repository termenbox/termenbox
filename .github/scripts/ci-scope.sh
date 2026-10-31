#!/bin/sh
# Reads the paths a pull request changes (one per line, stdin) and prints which parts of CI it can affect, as
# `name=true|false` lines for $GITHUB_OUTPUT:
#   code      false when nothing built, tested or shipped changes (docs and Markdown only): every job is skipped
#   scripted  false when the scripted-server browser matrix cannot notice the change: that matrix runs the built client
#             against fakes, never the server binary
# The lists below are the whole configuration. A path that matches neither is treated as code, and no paths at all (an
# empty or failed lookup) turns everything on, so a mistake here runs too much and never too little.
code=false
scripted=false
seen=0
while IFS= read -r path; do
  [ -n "$path" ] || continue
  seen=1
  case "$path" in
    docs/* | *.md) continue ;;
  esac
  code=true
  case "$path" in
    # vite.config.ts reads the version and the release keys from these two, so the client build depends on them
    packages/server/Cargo.toml | packages/server/release-keys.txt) scripted=true ;;
    packages/server/* | packages/agent/*) ;;
    *) scripted=true ;;
  esac
done
if [ "$seen" = 0 ]; then code=true; scripted=true; fi
echo "code=$code"
echo "scripted=$scripted"
