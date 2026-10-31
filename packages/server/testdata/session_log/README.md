# Session-log fixtures

Fixtures for the `session_log` adapters (decision 0157). Each agent directory holds the same six cases:
`short`, `rewind` (Claude) or `fork` (Codex), `torn`, `truncated`, `unknown-version`, `tool-secret`.

They are hand-built from session files measured on 2026-10-31 (Claude Code 2.1.308, Codex 0.166.0): the record
shapes, field names and noise records are the real ones, trimmed of system prompts, tool schemas and account ids.
All ids, paths and the secret (`sk-FAKE-...`) are fake. Fact 0065 lists what was measured and what was not.

Not measured, synthetic guesses to replace when the agent is next run interactively:
- `codex/fork.jsonl`: the `forked_from_id` field and the replay of the parent's records.
- `codex/tool-secret.jsonl`: the trailing `compacted` record.
- `claude/rewind.jsonl`: an abandoned branch built by hand (`-p` has no rewind); `--fork-session` was measured instead.
