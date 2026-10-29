# Bridge Contract crate in `packages/core`

Status legend: `[ ]` pending, `[~]` in progress, `[x]` done. Upstream plan: step D.1 of `bridge-contract-05-revision.md` in the Bridge Contract paper repository.

## Cold start

The paper's reference bridge (`bridge-contract` 0.5.0, Rust, standalone) models detach/resume as a product state machine `Attachment x Human x Lifecycle`, with a hash-chained governance log, an authenticated single-use resume token (I11) and restart recovery. Termenbox already detaches on a dropped connection (`workspaces.rs`: "a dropped connection only detaches") and re-attaches by workspace id. This plan makes `packages/core` consume the crate so those moments are governed by one tested machine, without changing a wire shape (guardrail `protocol-core-stays-acp-v1`).

## Steps

### 1. Vendor the crate — [x]
Check: `cargo test -p bridge-contract`
Copy to `packages/bridge-contract/` with the 20 shared vectors; local changes are listed in `VENDOR.md`. Workspace member; `serde_json`, `sha2` shared with the workspace. Done: all 20 vectors pass here.

### 2. Governor seam in core — [x]
Check: `cargo test -p termenbox-core governance`
`packages/core/src/governance.rs`: `Governor` wraps one workspace's bridge (attach, client disconnect, human wait/answer, tick, standing) and projects `HUMAN_PENDING` / `HUMAN_RESOLVED` / `HUMAN_TIMEOUT` onto the reducer's existing `Block` event. No other governance event enters the agent stream. Not wired into the server yet.

### 3-6. Durable log, Send, server wiring, records — [ ]
Superseded by `bridge-contract-acp-integration.md` (review of the ACP interaction design, mapping, wire shape and steps 5.1-5.8).

## Caveats

- The crate and the Python reference share an author; passing the vectors shows consistency of one reading, not independent confirmation (see `packages/bridge-contract/README.md`).
- Proof of possession in the crate is an Ed25519 signature and its registry holds public keys only (crate 0.6.0). `Bridge::attach` still takes a `ClientKey`, which carries the signing key; the Governor passes it through, so the server must not hold a client's real signing key once wiring starts (upstream change: attach by public key).
- The case study (D.2) needs real logs from step 5; none exist yet.
