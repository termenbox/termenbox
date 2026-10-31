#!/usr/bin/env python3
"""PreToolUse hook: no work in the main checkout; work happens in a pool worktree (scripts/pool.sh).

Blocks file edits whose path is inside a main checkout, and Bash commands that could change one. A main checkout is
a working tree whose git dir is the common git dir; a linked worktree is not. Reads are never blocked. Exit 2 blocks
and tells the agent what to do; any other exit allows. Fails closed when it cannot tell, but only inside a main
checkout: elsewhere nothing here applies.
"""
import json
import os
import re
import shlex
import subprocess
import sys

MSG = (
    "Blocked: this is the main checkout, and work happens only in a pool worktree "
    "(guardrail work-happens-in-pool-worktrees).\n"
    "Run `sh scripts/pool.sh acquire <type>/<slug>`, then work in the path it prints "
    "(`cd <path> && ...`, or start the session there). `sh scripts/pool.sh status` shows the pool."
)

# Commands that only read. A segment starting with anything else is refused in the main checkout.
READ_ONLY = {
    "ls", "cat", "head", "tail", "grep", "rg", "wc", "stat", "file", "echo", "pwd", "which", "diff", "tree",
    "du", "df", "date", "id", "whoami", "uname", "basename", "dirname", "realpath", "readlink", "true", "test", "[",
}
GIT_READ = {
    "status", "log", "diff", "show", "rev-parse", "rev-list", "remote", "fetch", "ls-files", "ls-tree", "blame",
    "describe", "for-each-ref", "cat-file", "shortlog", "merge-base", "name-rev", "grep", "reflog", "worktree",
    "branch", "tag", "config",
}
GIT_WRITING_FLAGS = {"-d", "-D", "-m", "-M", "-c", "-C", "--delete", "--move", "--copy", "--set-upstream-to",
                     "--unset-upstream", "--edit-description", "-f", "--force", "--set", "--unset", "--add",
                     "--replace-all", "--unset-all", "--rename-section", "--remove-section", "-e", "--edit"}
GH_READ = {("pr", "view"), ("pr", "list"), ("pr", "status"), ("pr", "checks"), ("pr", "diff"),
           ("issue", "view"), ("issue", "list"), ("issue", "status"), ("run", "view"), ("run", "list"),
           ("repo", "view"), ("auth", "status")}
POOL_SCRIPT = re.compile(r"^(sh |bash )?(\./)?scripts/pool\.sh$")


def toplevel_if_main(path):
    """The root of the main checkout containing `path`, or None (linked worktree, not a repository, no git)."""
    d = os.path.abspath(path)
    while d != "/" and not os.path.isdir(d):
        d = os.path.dirname(d)
    try:
        out = subprocess.run(
            ["git", "-C", d, "rev-parse", "--path-format=absolute", "--git-dir", "--git-common-dir", "--show-toplevel"],
            capture_output=True, text=True, timeout=5,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    lines = out.stdout.split("\n")
    if out.returncode != 0 or len(lines) < 3:
        return None
    git_dir, common, top = (os.path.realpath(x) for x in lines[:3])
    return top if git_dir == common else None


def block(why=None):
    sys.stderr.write(MSG + (f"\n({why})" if why else "") + "\n")
    sys.exit(2)


def in_main(path, base):
    p = os.path.expanduser(path)
    if not os.path.isabs(p):
        p = os.path.join(base, p)
    return toplevel_if_main(os.path.normpath(p)) is not None


def split_segments(cmd):
    """Split on ; && || | and newlines outside quotes. None when it holds a substitution or a background job."""
    if "`" in cmd or "$(" in cmd or "<(" in cmd or ">(" in cmd:
        return None
    segs, cur, quote, i = [], "", None, 0
    while i < len(cmd):
        c = cmd[i]
        if quote:
            cur += c
            if c == quote:
                quote = None
        elif c in "'\"":
            quote = c
            cur += c
        elif c in ";\n|":
            segs.append(cur)
            cur = ""
        elif c == "&":
            if cmd[i:i + 2] == "&&":
                segs.append(cur)
                cur = ""
                i += 1
            else:
                return None
        else:
            cur += c
        i += 1
    segs.append(cur)
    return [s.strip() for s in segs if s.strip()]


def words(seg):
    try:
        return shlex.split(seg)
    except ValueError:
        return None


def read_only(seg, cwd):
    """True when this one simple command cannot change the main checkout."""
    w = words(seg)
    if not w:
        return False
    if ">" in seg:  # any redirection
        return False
    first = w[0]
    if first in ("sh", "bash") and len(w) >= 2:
        return bool(POOL_SCRIPT.match(f"{first} {w[1]}"))
    if POOL_SCRIPT.match(first):
        return True
    if first == "git":
        i = 1
        while i < len(w) and w[i].startswith("-"):
            if w[i] == "-C" and i + 1 < len(w):
                if not in_main(w[i + 1], cwd):
                    return True  # git in a worktree
                i += 2
                continue
            if w[i] == "-c":
                return False
            i += 1
        if i >= len(w):
            return True
        sub, rest = w[i], w[i + 1:]
        if sub not in GIT_READ:
            return False
        if sub == "worktree":
            return bool(rest) and rest[0] == "list"
        if sub == "remote":
            return not rest or rest[0] in ("-v", "show", "get-url")
        if sub == "config":
            return any(a in ("--get", "--get-all", "--list", "-l", "--get-regexp") for a in rest)
        if sub in ("branch", "tag"):
            return not any(a in GIT_WRITING_FLAGS for a in rest) and all(a.startswith("-") for a in rest)
        return True
    if first == "gh":
        return len(w) >= 3 and (w[1], w[2]) in GH_READ
    if first == "find":
        return not any(a in ("-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprintf", "-fls") for a in w)
    return first in READ_ONLY


def check_bash(cmd, cwd):
    if toplevel_if_main(cwd) is None:
        return  # the session works in a worktree or outside the repository
    segs = split_segments(cmd)
    if not segs:
        block("cannot verify an empty command, a substitution or a background job")
    first = words(segs[0]) or []
    moves = [s for s in segs if (words(s) or [""])[0] in ("cd", "pushd", "popd")]
    if first[:1] == ["cd"] and len(first) >= 2:
        if len(moves) == 1 and not in_main(first[1], cwd):
            return  # the whole command runs in a worktree
        block("a cd into a worktree must be the only directory change")
    if moves:
        block("directory changes are only allowed to enter a worktree")
    if all(read_only(s, cwd) for s in segs):
        return
    block("only read-only commands run in the main checkout")


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return  # not ours to judge
    tool = event.get("tool_name", "")
    args = event.get("tool_input") or {}
    cwd = event.get("cwd") or os.getcwd()
    if tool in ("Edit", "Write", "NotebookEdit", "MultiEdit"):
        path = args.get("file_path") or args.get("notebook_path") or ""
        if path and in_main(path, cwd):
            block(path)
    elif tool == "Bash":
        check_bash(args.get("command", ""), cwd)


main()
