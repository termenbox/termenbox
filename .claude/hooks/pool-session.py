#!/usr/bin/env python3
"""SessionStart / SessionEnd hook: a session started in a main checkout takes a pool slot by itself.

SessionStart in a main checkout runs `scripts/pool.sh session <id>` (a free slot on origin/develop, unnamed) and
tells the agent to work there and to name the branch with `pool.sh name <type>/<slug>` once the task is clear.
SessionEnd gives back a slot that is still unnamed and clean. A session already in a linked worktree is left
alone. Like worktree-only.py it acts only where TERMENBOX_POOL_ONLY=1, and it never fails the session.
"""
import json
import os
import subprocess
import sys

CONTEXT = (
    "This session started in the main checkout, where work is not allowed (guardrail work-happens-in-pool-worktrees). "
    "Your worktree for this session is {path}, already on origin/develop. Do all work there: your first command is "
    "`cd {path}`, and file paths you edit are under {path}. Do not create or switch branches until the task is clear. "
    "Once it is, pick a branch name `<type>/<slug>` (type one of feat, fix, docs, chore, ci, test, refactor, perf, "
    "build, style, revert; slug lowercase with dashes), suggest it to the user in one line, and run "
    "`sh scripts/pool.sh name <type>/<slug>` inside the worktree. Merged slots free themselves."
)


def git(cwd, *args):
    out = subprocess.run(["git", "-C", cwd, *args], capture_output=True, text=True, timeout=5)
    return out.stdout.strip() if out.returncode == 0 else None


def in_main_checkout(cwd):
    git_dir = git(cwd, "rev-parse", "--path-format=absolute", "--git-dir")
    common = git(cwd, "rev-parse", "--path-format=absolute", "--git-common-dir")
    return bool(git_dir) and git_dir == common


def main():
    if os.environ.get("TERMENBOX_POOL_ONLY") != "1":
        return
    try:
        event = json.load(sys.stdin)
        cwd = event.get("cwd") or os.getcwd()
        session = event.get("session_id") or ""
        if not session or not in_main_checkout(cwd):
            return
        script = os.path.join(git(cwd, "rev-parse", "--show-toplevel"), "scripts", "pool.sh")
        if not os.path.isfile(script):
            return
        if event.get("hook_event_name") == "SessionEnd":
            subprocess.run(["sh", script, "end-session", session], cwd=cwd, capture_output=True, timeout=60)
            return
        out = subprocess.run(["sh", script, "session", session], cwd=cwd, capture_output=True, text=True, timeout=120)
        path = out.stdout.strip().splitlines()[-1] if out.returncode == 0 and out.stdout.strip() else ""
        if path:
            print(json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart",
                                                     "additionalContext": CONTEXT.format(path=path)}}))
    except (OSError, ValueError, subprocess.SubprocessError):
        return


main()
