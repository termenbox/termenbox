---
id: 0057
title: ACP's own extension rules and what the pinned schema crate covers, checked for the extension registry
status: active
tags: [agents, protocol, client]
date: 2026-10-25
kind: environmental
governed-by: 0136
last-verified: 2026-10-25
---

# ACP's own extension rules and what the pinned schema crate covers, checked for the extension registry

Decision 0136 builds its extension registry on ACP's `_meta` and custom methods. Checked on 2026-10-25 against the ACP specification page <https://agentclientprotocol.com/protocol/extensibility> and `agent-client-protocol-schema` **1.12.0** (the version `packages/server/Cargo.toml` requires with `default-features = false`; `Cargo.lock` has the same). The round-trip and unknown-value rows were run, not read: a throwaway crate against the registry source printed them.

## What the specification says

| Question | Answer |
|---|---|
| Where may an extension put data? | In the `_meta` field of any object type that defines one. "Implementations MUST NOT add any custom fields at the root of a type that's part of the specification." |
| How is `_meta` namespaced? | No formal rule. The page's example keys are domain-prefixed (`zed.dev/debugMode`). Only `traceparent`, `tracestate` and `baggage` are reserved (W3C trace context, SHOULD). |
| Custom methods | Any method name starting with `_` is reserved for extensions, as requests or as notifications. |
| What must a peer do with an unknown one? | An unknown request gets the standard "Method not found" error. Unknown notifications SHOULD be ignored. |
| How are extensions advertised? | SHOULD be in the `_meta` of the capability objects in `initialize` (example: `agentCapabilities._meta."zed.dev"`), so the caller checks before use. |

## What the pinned crate does (v1 types)

| Question | Answer |
|---|---|
| Is `_meta` kept on a round trip? | Yes. `Meta` is `serde_json::Map<String, Value>`; a `SessionUpdate::AgentMessageChunk` with `_meta` on the update and on its text block came back byte for byte, and so did `_meta` under `AgentCapabilities`. |
| Is an unknown field at the root of a type kept? | **No, it is dropped silently** (`"seq":9` beside a known update vanished on re-serialising). A registry that wants a value to survive must use `_meta`; anything server-side that deserialises and re-serialises a core type loses root extras. |
| Is an unknown `sessionUpdate` kind skipped? | **No, it fails to decode**: `unknown variant ... expected one of user_message_chunk, agent_message_chunk, agent_thought_chunk, tool_call, tool_call_update, plan, available_commands_update, current_mode_update, config_option_update, session_info_update, usage_update, notice, compaction_update, compaction_summary_chunk`. The enum is `#[non_exhaustive]` for Rust callers but has no catch-all for the wire. |
| Is an unknown content block `type` skipped? | **No, it fails to decode** (`unknown variant hologram, expected text, image, audio, resource_link, resource`). Same for any other internally tagged enum. |
| Custom methods | `ExtRequest`, `ExtResponse` and `ExtNotification` carry a method name and raw JSON params (`Arc<RawValue>`); they are the `_`-prefixed route. |
| `session/update` kinds covered | The 14 above. `plan_update` and `plan_removed` exist only behind `unstable_plan_operations`. 1.12.0 also has a `v2` module (`unstable_protocol_v2`); termenbox uses `v1`. |
| Content blocks | `text`, `image`, `audio`, `resource_link`, `resource` (embedded). |
| Terminals | Client methods `terminal/create`, `terminal/output`, `terminal/release`, `terminal/wait_for_exit`, `terminal/kill`, a `TerminalId`, and a tool-call content variant `Terminal { terminal_id }`. |
| Other methods typed | `initialize`, `authenticate`, `session/new|load|resume|close|list|delete|prompt|cancel|set_mode|set_config_option`, `session/request_permission`, `fs/read_text_file`, `fs/write_text_file`, `elicitation/create`, `$/cancel_request`. `session/fork`, `providers/*` and `mcp/message` are behind unstable features. |
| Protocol version | `ProtocolVersion::LATEST` prints `1`. |

## Conflicts with the rule "unknown is skipped"

1. **The schema types do not skip unknown update kinds or content types.** Skipping must happen one layer up: look at the raw `sessionUpdate` / `type` string, hand a known one to the typed decoder and pass an unknown one on as a generic item. The server's lenient per-message decoding (decision 0111) already works this way; the client reducer of step 5.1 needs the same.
2. **Unknown root fields vanish.** An extension on a core object must live in `_meta` under a domain-prefixed key (for example `termenbox.dev/seq`), never beside the standard fields, which the specification forbids anyway.
3. **Unknown requests are errors, unknown notifications are ignored.** An extension request that a peer lacks fails with "Method not found", so the registry must negotiate at `initialize` (capability `_meta`) before sending one, and prefer notifications where a lost message is harmless.
4. The specification gives no namespacing rule beyond examples; the prefix `termenbox.dev/` is termenbox's own convention, not ACP's.

Not established: what other ACP agents do with an unknown `_meta` key in a capability object (they were not probed; the crate keeps it, which is all a Rust peer needs). Facts 0038 and 0040 hold the per-agent measurements.
