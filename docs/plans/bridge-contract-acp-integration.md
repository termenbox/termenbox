# Bridge Contract on the ACP path: review and integration plan

Status legend: `[ ]` pending, `[~]` in progress, `[x]` done. Builds on `bridge-contract-crate.md` (steps 1-2 done: vendored crate, `termenbox-core::governance::Governor`). This plan replaces that plan's steps 3-6. Read decisions 0136, 0139, 0138, guardrails `protocol-core-stays-acp-v1`, `kill-session-needs-confirmation`, `security-secrets-handling`, `audit-logging-no-secrets`.

## 1. How ACP interaction works today

Two hops, only one of them is ACP on the wire.

| Hop | Wire today | Where |
|---|---|---|
| agent <-> server | ACP v1 over stdio; the server is the ACP *client*, the agent a spawned child | `acp.rs` (`Live`, `UpdateMapper`) |
| server <-> browser | custom `acp_open` / `acp_view` / `acp_answer` messages; moving to ACP v1 + `termenbox.dev/*` extensions (0136, plan `workspace-protocol`) | `workspaces.rs`, `acp_stream.rs`, `ext.rs` |

Facts that matter for governance:

- **The agent is independent of the client.** A browser drop only sets the connection's `acp` subscription to `None` (`workspaces.rs`); the child keeps running and `Stream` keeps accumulating. There is no detach state, deadline or event.
- **Resume is a view cursor, not an authorisation.** `acp_open{after, epoch}` replays the tail of the `Stream` ring (`Stream::view`; `reset` when the epoch changed or `after` fell off). Authority to resume is the connection's own authentication (GitHub-certified device key signing a per-connection challenge, `cert.rs`, decision 0062). Nothing binds a re-attach to the last state the client saw, and nothing is single-use.
- **Permissions wait forever.** `session/request_permission` becomes a `Perm{rpc_id, allowed}` in `Inner.perms` and a `Block` event; the JSON-RPC request stays open until `answer_permission`. Bounds: `MAX_PENDING = 8` (overflow answers `cancelled`). No time limit, no record of the decision beyond the audit log lines and the stream. A permission asked while nobody is attached just sits there.
- **Authority has no scope model.** Options are `allow_once` / `reject_once` only (0104); modes (`set_mode`) can widen what the agent may do, with no check against what the owner granted at attach.
- **Restart loses the conversation of intent.** `Saved{acp_id, turn_open, ...}` is rewritten on turn change; `restore()` uses `session/resume|load`. Pending permissions die with the child (`turn_open` says only that a turn was interrupted). The `Stream` epoch changes, so clients get a `reset`.
- **Persistence is the product promise.** Workspaces outlive connections indefinitely (tmux and ACP alike).

## 2. Mapping

Termenbox's server **is** the Bridge: it sits between the agent runtime (the thing that acts) and the human's client (the thing that authorises).

| Bridge Contract | Termenbox |
|---|---|
| session `sid` | workspace id (the ACP workspace; one governance session per ACP `sessionId` once 0138 lands) |
| principal | `Identity{provider, id}` of the owner connection |
| client key, proof of possession (I11) | the **device key** (ECDSA P-256, non-extractable): it already signs a per-connection challenge; the bridge nonce is signed the same way. No new browser key. |
| `ATTACHED` | first `acp_open` of a workspace by a new connection when no governance session exists |
| `client_disconnect` -> `Det` | the last connection showing the workspace closes or goes silent |
| resume token R, `begin_resume`/`complete_resume` | `acp_open` carrying the token and proof (see 3); `complete_resume` runs before the view is sent |
| `escalate_to_human` / `human_resolve` | `on_permission` (first open perm) / `answer_permission` (last open perm answered or cancelled); already mirrored by `Block` events |
| tool queue, `T_call`, `q_max` (I13) | open `Perm`s: `q_max` replaces `MAX_PENDING`; `T_call` is a new expiry that answers `cancelled` |
| narrow-only scope (I3, I10) | modes and permission tiers: `set_mode` to a more permissive mode is an *extension attempt* and needs a fresh grant; narrowing is always allowed |
| `TERMINATION` | **not** a kill. It closes the governance session: open perms answered `cancelled`, prompts refused, the agent stopped only with the owner's confirmation (`kill-session-needs-confirmation`). A new attach (a fresh grant) opens a new governance session. |
| durable log, recovery (I12) | per-workspace append-only file beside the ACP state file; `restore()` calls `Bridge::recover` before `session/resume` |
| `T_detach` | owner-configurable; **default off or long** (see 4), because bounded detach contradicts the persistence promise |

Not governed: terminal bytes and tmux workspaces (no agent decisions flow through the bridge). A later step may govern hook-adapter agents the same way, through the same ports (0136).

## 3. Wire shape

Stage A (now, before the ACP-wire migration lands): optional fields, ignored by old peers, as `after`/`epoch` were added.
- `acp_open` gains `resume: {token, proof}` (absent on a first attach).
- `acp_view` (first part) gains `governance: {state, token?, nonce}` where `token` is the freshly issued R after `complete_resume`, `nonce` the challenge for the next re-attach.

Stage B (with the ACP wire, 0136): the same two objects ride as extension `termenbox.dev/resume` (version 1) in `_meta` of the open/initialize exchange, negotiated through `ext::Registry`. A client or agent that does not list it is spoken to in plain ACP: the bridge then runs with the *implicit* client of the connection and no resume token (degraded, stated in the `tier`-style capability, never silently). Stripping the extension must leave valid ACP (`protocol-core-stays-acp-v1`). The token's `pending_tool_calls` carries prompt ids only, never tool text (`notifications-carry-no-agent-text`, `audit-logging-no-secrets`).

Token and view cursor stay separate: the token authorises (control plane, I4/I11), `(epoch, rev)` replays data (0139). A valid token with a stale cursor gets a compacted `reset`; a stale token with a valid cursor is rejected.

## 4. Conflicts to settle with the owner before step 5

1. **`T_detach` vs persistence.** Crate defaults (`T_detach` 30 min, `T_token` 15 min) terminate a phone that is away for 31 minutes. Proposed: `T_detach` default 7 days (config `TERMENBOX_GOV_DETACH`), `T_token = T_detach` is not required but `T_token <= T_detach` is; and since a token older than `T_token` ends the session (I9, `begin_resume`), re-issue the token on every `tick` while detached, bounded by `T_detach`. This needs a **crate change** (rolling token), not a Termenbox workaround.
2. **MAC key custody.** The crate needs a bridge-only key that survives restart; `security-secrets-handling` says secrets stay in memory. Proposed: derive it with HKDF from the server's persisted signing key (already on disk for pairing/directory), label `termenbox.dev/bridge-mac/v1`; nothing new is stored. Amend the guardrail text to name this derivation.
3. **Proof algorithm.** The crate's HMAC stand-in becomes a verifier callback; Termenbox supplies ECDSA P-256 verification (`cert.rs::verify_challenge` already does raw r||s over a message). Ed25519 is not an option for the client key: WebCrypto Ed25519 needs Safari 17, the floor is 16.4 (decision 0112).
4. **Mode changes as authority.** Decide which modes widen. Start with: any change to a mode the agent lists as permission-bypassing needs a fresh grant; everything else is not scope.

## 5. Steps

### 5.1 Upstream crate changes — [ ]
Class: implement (in the paper repo) · Needs: owner decisions 4.1-4.3 · Check: `cd paper/supplementary/rust-bridge && cargo test` there, then re-vendor
Do: (a) `Storage` trait + `FileStorage`-friendly API, `Send + Sync` (`Arc`, `Mutex`), `Clock: Send + Sync`; (b) rolling resume token while detached; (c) proof verifier trait (`fn verify(&self, key_id, nonce, seq, proof) -> bool`) with the HMAC one as the test default; (d) a vector for each (rolling token, bad-algorithm proof). Keep the independence caveats in the README; bump to 0.5.1; update `VENDOR.md`.
Done-when: vectors pass on Python and Rust; vendored copy matches upstream except the listed local changes.

### 5.2 Durable governance log — [ ]
Class: implement · Needs: 5.1 · Check: `cargo test -p termenbox-server governance_log`
Do: `packages/server/src/governance.rs`: file storage (one append-only file per workspace under the state dir, fsync per batch, 0600), MAC key via HKDF (4.2), recovery at `AcpBackend::restore()` before `start()`. A workspace whose log fails verification is not restored silently: it shows "Stopped: governance log failed verification" in the existing `Entry::stopped` path.
Done-when: kill -9 during a transition leaves the log at a transition boundary; restart recovers the same cell.

### 5.3 Attach, detach and human wait — [ ]
Class: implement · Needs: 5.2 · Check: `cargo test -p termenbox-server`
Do: `Live` holds a governor; hook `acp_open` (attach / resume), connection close (`client_disconnect`), `on_permission` / `answer_permission` / `cancel_pending` (`escalate_to_human` / `human_resolve`), `tick` from the existing 1 s loop. Replace `MAX_PENDING` with the bridge `q_max`; add `T_call` expiry answering `cancelled`. Emit audit lines (`event = "gov_*"`, no text) for each committed event kind. Stage A wire fields in `protocol.rs` with round-trip tests that old messages still parse.
Done-when: scripted test: drop connection -> ask permission -> reconnect with token -> permission still open and answerable; wrong proof -> `RESUME_REJECTED`, no view sent.

### 5.4 Client — [ ]
Class: implement · Needs: 5.3 · Check: `pnpm -r test` (via `scripts/dev.sh --full`)
Do: `AcpView` store keeps the latest token per workspace in memory (and IndexedDB for a reload, bound to the device key); sign the nonce with the device key; send `resume` on `acp_open`; on `RESUME_REJECTED` fall back to a fresh attach that asks the owner (never silently widens).
Done-when: reload and phone-sleep paths in the e2e suite resume without a prompt; a tampered token shows the fresh-attach prompt.

### 5.5 Narrow-only modes — [ ]
Class: implement · Needs: 5.3, owner decision 4.4 · Check: `cargo test -p termenbox-server mode_widening`
Do: `set_mode` calls `attempt_authority_extension` first; a widening is refused with a visible notice unless the owner re-grants through the confirmation flow.

### 5.6 Extension stage B — [ ]
Class: implement · Needs: 5.3 and the ACP-wire migration (`workspace-protocol` phase 3) · Check: `cargo test -p termenbox-core ext` and the stripped-stream CI check
Do: register `termenbox.dev/resume` v1 in `ext.rs` (shared vector in `testdata/extension-vectors.json`, TypeScript side in `extensions.ts`); keep Stage A fields for one release window, then drop them.

### 5.7 Decision, guardrails, fact — [ ]
Class: implement · Needs: 5.3 · Check: `go-getter lint`
Do: decision "Detach and resume are governed by the Bridge Contract" (amends 0136/0139 on resume); guardrail "every re-attach of an agent workspace passes `complete_resume`; a failure never falls back to an ungoverned view"; guardrail amendment for the MAC-key derivation; fact with the configured defaults and their rationale.

### 5.8 Measure for the case study — [ ]
Class: explore · Needs: 5.3 · Check: `test -s docs/plans/assets/governance-log-sample.jsonl`
Do: log detach/resume/permission incidents from real use with the owner's consent (COI disclosed: the owner authors both paper and product); this feeds paper step D.2. Sanitise: ids and reasons only.

## 6. Risks

- **Overgovernance.** A rejected resume on a flaky phone link is a worse product than today. Mitigation: rejection reasons are specific, the fallback is one tap, and `T_detach` defaults long.
- **Two sources of truth for "waiting on human".** `Block` events and bridge `Human` must not diverge. Mitigation: the bridge cell is derived from the same calls (5.3), and the `Governor` projection (already built) feeds `Block`, so the hub never sees a block the bridge did not commit.
- **Same-author evidence.** The crate, the Python reference and this integration share an author; the case study (5.8) is the first independent-in-practice check and must report incidents where the governance got in the way.
- **Scope of claims.** Only ACP workspaces (T0) are governed in this plan; say so wherever the feature is described.
