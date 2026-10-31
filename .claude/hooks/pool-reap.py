#!/usr/bin/env python3
"""SessionStart / PostToolUse hook: give back pool slots whose branch is merged (scripts/pool.sh reap).

Runs at the start of a session and after a Bash command that merges a pull request, so finished slots free
themselves without anyone releasing them. Detached and silent: a hook never delays or fails the session. Like
worktree-only.py it acts only where TERMENBOX_POOL_ONLY=1.
"""
import json
import os
import re
import subprocess
import sys


def main():
    if os.environ.get("TERMENBOX_POOL_ONLY") != "1":
        return
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return
    if event.get("hook_event_name") == "PostToolUse":
        command = (event.get("tool_input") or {}).get("command", "")
        if event.get("tool_name") != "Bash" or not re.search(r"\bgh\s+pr\s+merge\b", command):
            return
    cwd = event.get("cwd") or os.getcwd()
    script = os.path.join(os.environ.get("CLAUDE_PROJECT_DIR", cwd), "scripts", "pool.sh")
    if not os.path.isfile(script):
        return
    subprocess.Popen(
        ["sh", script, "reap"], cwd=cwd, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL, start_new_session=True,
    )


main()
