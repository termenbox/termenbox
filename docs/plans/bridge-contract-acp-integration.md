# Bridge Contract on the ACP path: review and integration plan

Status legend: `[ ]` pending, `[~]` in progress, `[x]` done. Builds on `bridge-contract-crate.md` (steps 1-2 done: vendored crate, `termenbox-core::governance::Governor`). This plan replaces that plan's steps 3-6. It depends on spec changes in the paper repository, listed in its `docs/plans/bridge-contract-integration-findings.md` (E.1-E.9); where this plan says "upstream", that is meant. Read decisions 0136, 0139, 0138, guardrails `protocol-core-stays-acp-v1`, `kill-session-needs-confirmation`, `security-secrets-handling`, `audit-logging-no-secrets`.

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
| client keys, proof of possession (I11) | the owner's **device keys** (ECDSA P-256, non-extractable, one per certified device): each already signs a per-connection challenge; the bridge nonce is signed the same way. The key set belongs to the principal (upstream E.2). No new browser key. |
| `ATTACHED` | first `acp_open` of a workspace by a new connection when no governance session exists |
| `client_disconnect` -> `Det` | the **last** connection showing the workspace closes or goes silent (several devices may be attached; upstream E.2) |
| resume (challenge, `begin_resume`/`complete_resume`) | `acp_open` carrying nonce, last-seen `(seq, hash)` and proof (see 3); `complete_resume` runs before the view is sent. No token is held by the client (upstream E.1). |
| `escalate_to_human` / `human_resolve` | `on_permission` (first open perm) / `answer_permission` (last open perm answered or cancelled); already mirrored by `Block` events |
| tool queue, `T_call`, `q_max` (I13) | open `Perm`s: `q_max` replaces `MAX_PENDING`; `T_call` is a new expiry that answers `cancelled` |
| narrow-only scope (I3, I10) | modes and permission tiers: `set_mode` to a more permissive mode is an *extension attempt* and needs a fresh grant; narrowing is always allowed |
| `TERMINATION` | **not** a kill. It closes the governance session: open perms answered `cancelled`, prompts refused, the agent stopped only with the owner's confirmation (`kill-session-needs-confirmation`). A new attach (a fresh grant) opens a new governance session. |
| durable log, recovery (I12) | per-workspace append-only file beside the ACP state file; `restore()` calls `Bridge::recover` before `session/resume` |
| `T_detach` | **7 days** (owner decision), configurable; `T_human` defaults to `T_detach`, `T_call` is owner-visible (see 4) |

Not governed: terminal bytes and tmux workspaces (no agent decisions flow through the bridge). A later step may govern hook-adapter agents the same way, through the same ports (0136).

## 3. Wire shape

Why not a held token: the contract's token is minted when the client detaches and again on every later human-wait change, so an abruptly dropped phone never holds the current one (finding 1 in the paper plan). Resume is therefore challenge-response, pinned to what the client last saw.

Flow (upstream E.1):
1. The client keeps `(gseq, ghash)`: the last governance event it saw, carried in `acp_view` beside `(epoch, rev)`.
2. To reconnect it sends `acp_resume_begin{workspace, key_id}`; the server answers `nonce` (random, 60 s, single use).
3. The client sends `acp_open{workspace, after, epoch, resume:{nonce, gseq, ghash, proof}}`; `proof` is the device key's signature over `(server_id, workspace, nonce, gseq, ghash)` (same shape as `cert.rs::challenge_message`).
4. The server accepts iff the signature verifies against a key of the principal, the nonce is live and unused, and `ghash` equals the hash of the event at `gseq` in its chain. Events after `gseq` (scope changes, human waits) arrive in the view; the client reconciles. Otherwise `RESUME_REJECTED{reason}` and no view.

Stage A (before the ACP wire migration): `resume_begin` and `nonce` are new messages, `resume` an optional field on `acp_open`, `governance:{gseq, ghash, standing}` an optional field on `acp_view`; old peers ignore them, as they ignored `after`/`epoch`.

Stage B (with the ACP wire, 0136): the same objects ride as extension `termenbox.dev/resume` v1 (`_meta`, plus the method `_termenbox.dev/resume_begin`), negotiated through `ext::Registry`. A peer without it is spoken to in plain ACP and the bridge runs with the connection's own authenticated client and no resume challenge (degraded, stated in the capability, never silent). Stripping the extension leaves valid ACP (`protocol-core-stays-acp-v1`). Governance events carry ids, hashes and reasons only, never tool text (`notifications-carry-no-agent-text`, `audit-logging-no-secrets`).

The proof authorises (control plane); `(epoch, rev)` replays data (0139). A valid proof with a stale cursor gets a compacted `reset`; a mismatching `ghash` is rejected.

## 4. Decisions

1. **`T_detach` = 7 days** (owner, 2026-10-25). With challenge-response there is no token TTL to couple to it. `T_human` defaults to `T_detach`, so a permission left unanswered does not end the session after 24 h. `T_call` (an open permission is answered `cancelled` after it) is shown to the owner in settings; default proposed 24 h, owner to confirm. Config: `TERMENBOX_GOV_DETACH`, `_HUMAN`, `_CALL`.
2. **MAC key custody** (proposed, open): HKDF from the server's persisted signing key, label `termenbox.dev/bridge-mac/v1`; nothing new on disk. Amend `security-secrets-handling` to name the derivation.
3. **Proof algorithm** (proposed, open): a `Verifier` trait in the crate (upstream E.7); Termenbox supplies ECDSA P-256 (`cert.rs`). Ed25519 is not the client key: WebCrypto Ed25519 needs Safari 17, the floor is 16.4 (0112).
4. **Mode widening** (proposed, open): bypass modes only at first. The owner's `set_mode` is *enforced* (refused without a fresh grant); an agent's own mode update is *observed*: log `AUTHORITY_EXTENSION_REJECTED` and send `session/set_mode` back to the granted mode (upstream E.4).

## 5. Steps

### 5.1 Upstream spec and crate changes — [ ]
Class: implement (in the paper repo) · Needs: owner decisions 4.2-4.4 · Check: in the paper repo, `cd paper/supplementary && python3 -I run_vectors.py && (cd rust-bridge && cargo test)`
Do: paper plan steps E.1-E.7 (mint-on-reconnect with prefix pinning, multiple clients, answer binding, coverage and I3 split, timer derivation, vectors/TLC/mutants, crate API: `Storage` trait, `Send + Sync`, `Verifier` trait). Then re-vendor here and update `VENDOR.md`.
Done-when: the vendored crate passes the upstream vectors and exposes a `Send` bridge over a pluggable storage.

### 5.2 Durable governance log — [ ]
Class: implement · Needs: 5.1 · Check: `cargo test -p termenbox-server governance_log`
Do: `packages/server/src/governance.rs`: file storage (one append-only file per workspace under the state dir, fsync per batch, 0600), MAC key via HKDF (4.2), recovery at `AcpBackend::restore()` before `start()`. A workspace whose log fails verification is not restored silently: it shows "Stopped: governance log failed verification" in the existing `Entry::stopped` path.
Done-when: kill -9 during a transition leaves the log at a transition boundary; restart recovers the same cell.

### 5.3 Attach, detach and human wait — [ ]
Class: implement · Needs: 5.2 · Check: `cargo test -p termenbox-server`
Do: `Live` holds a governor with a client set (one entry per attached connection; detach only when the set empties); hook `acp_open` (attach / resume), connection close, `on_permission` / `answer_permission` / `cancel_pending` (`escalate_to_human` / `human_resolve`), `tick` from the existing 1 s loop. Replace `MAX_PENDING` with the bridge `q_max`; add `T_call` expiry answering `cancelled`. Bind each answer to the content shown: store `h(prompt_id, options, tool title)` at `on_permission` and require it on `acp_answer` (upstream E.3); write the governance event before `respond` (event before ack). Add a per-workspace cap on permission requests per minute (one event, one push each) beside `q_max`. Emit audit lines (`event = "gov_*"`, no text) for each committed event kind. Stage A wire fields in `protocol.rs` with round-trip tests that old messages still parse.
Done-when: scripted test: drop connection -> ask permission -> reconnect with a signed nonce -> permission still open and answerable; wrong proof -> `RESUME_REJECTED`, no view sent.

### 5.4 Client — [ ]
Class: implement · Needs: 5.3 · Check: `pnpm -r test` (via `scripts/dev.sh --full`)
Do: `AcpView` store keeps `(gseq, ghash)` per workspace (memory, and IndexedDB for a reload); on reconnect request a nonce, sign it with the device key, send `resume` on `acp_open`; on `RESUME_REJECTED` fall back to a fresh attach that asks the owner (never silently widens). A second device simply attaches; it needs no token from the first.
Done-when: reload and phone-sleep paths in the e2e suite resume without a prompt; a tampered `ghash` shows the fresh-attach prompt.

### 5.5 Narrow-only modes — [ ]
Class: implement · Needs: 5.3, owner decision 4.4 · Check: `cargo test -p termenbox-server mode_widening`
Do: the owner's `set_mode` calls `attempt_authority_extension` first; a widening is refused with a visible notice unless the owner re-grants through the confirmation flow. An agent-announced mode update that widens is logged and reverted with `session/set_mode` (observed, not prevented; say so in the UI text).

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
- **Scope of claims.** Only ACP workspaces (T0) are governed in this plan, and only calls that pass `session/request_permission`: tools an agent runs without asking are outside the contract. Say so wherever the feature is described.
- **Multi-device races.** Two devices answering the same permission: the first answer wins, the second gets "no longer open" (existing behaviour); the answer-binding hash makes sure each saw the same content.

## 7. Proposals considered and not taken

| Proposal | Decision | Reason |
|---|---|---|
| Resume token encryption envelope | skip | Transport is encrypted; the client holds no token. |
| Bridge operator attestation (I-ATTEST) | skip | One owner runs their own server; releases are already signed. |
| Concurrent session coordination | later | A group scope will matter with several agent windows per workspace (0138); separate spec. |
| Policy enforcement hooks | later | Real need (agent blocked on permissions while detached) but it is widening; must be a pre-granted narrow scope, local rules rather than OPA. |
| QUIC 0-RTT replay (I8) | skip | No QUIC; the single-use nonce covers reconnect replay. |
| I-SIGN-JWKS | n/a | The client pins the server key. |
| I-SIGN-EVENT / checkpoints | later, cheap | Sign checkpoints with the server Ed25519 key so a client can detect a rolled-back server; also bounds log growth. |
| Five-state vocabulary | skip | The product model is exposed through the existing `Activity` / `Attention` state, not a second state machine. |
