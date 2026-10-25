# Bridge Contract crate in `packages/core`

Status legend: `[ ]` pending, `[~]` in progress, `[x]` done. Upstream plan: step D.1 of `bridge-contract-05-revision.md` in the Bridge Contract paper repository.

## Cold start

The paper's reference bridge (`bridge-contract` 0.5.0, Rust, standalone) models detach/resume as a product state machine `Attachment x Human x Lifecycle`, with a hash-chained governance log, an authenticated single-use resume token (I11) and restart recovery. Termenbox already detaches on a dropped connection (`workspaces.rs`: "a dropped connection only detaches") and re-attaches by workspace id. This plan makes `packages/core` consume the crate so those moments are governed by one tested machine, without changing a wire shape (guardrail `protocol-core-stays-acp-v1`).

## Steps

### 1. Vendor the crate — [x]
Check: `cargo test -p bridge-contract`
Copy to `packages/bridge-contract/` with the 20 shared vectors; local changes are listed in `VENDOR.md`. Workspace member; `hmac`, `serde_json`, `sha2` shared with the workspace. Done: all 20 vectors pass here.

### 2. Governor seam in core — [x]
Check: `cargo test -p termenbox-core governance`
`packages/core/src/governance.rs`: `Governor` wraps one workspace's bridge (attach, client disconnect, human wait/answer, tick, standing) and projects `HUMAN_PENDING` / `HUMAN_RESOLVED` / `HUMAN_TIMEOUT` onto the reducer's existing `Block` event. No other governance event enters the agent stream. Not wired into the server yet.

### 3. Durable storage — [ ]
Check: `cargo test -p bridge-contract durable`
The vendored `Storage` is an in-memory struct behind `Rc<RefCell>`; axiom A1 needs a durable append that is atomic per transition (I12) and survives restart (`Bridge::recover`). Termenbox has a journal (`journal.rs`) and per-workspace state files. Do: raise upstream a `Storage` trait (append_batch, read log, client keys), implement it over a state file with fsync, and re-vendor. Do not fork `src/` here.

### 4. `Send` — [ ]
Check: `cargo check -p termenbox-server`
`Bridge` is `!Send` (`Rc`). The server is tokio multi-thread, so a `Governor` cannot sit in a task state shared across `.await` on another thread. Options: ask upstream for `Arc<Mutex<..>>`/`Send` storage and clock (preferred), or run governors on one dedicated thread behind a channel. Decide with the owner before step 5.

### 5. Wire detach/attach in the server — [ ]
Check: `cargo test -p termenbox-server`
In `workspaces.rs`, on `ClientMessage::Attach` for a workspace with a governor in `Detached`, run `begin_resume` / `complete_resume` instead of a bare re-attach; on a drop call `client_disconnected`; on ACP permission requests call `human_wanted` / `human_answered`. A rejected resume must fall back to a fresh attach, never to an unauthenticated one. Resume token and proof travel as a namespaced extension negotiated at `initialize` (0136), skipped by peers that do not know it.

### 6. Decision and guardrail — [ ]
Record a decision (governed detach/resume via the Bridge Contract) and, once step 5 lands, a guardrail that every re-attach goes through `complete_resume`. Add the config defaults (`T_detach` 30 min, `T_token` 15 min, `T_token <= T_detach`) to a fact.

## Caveats

- The crate and the Python reference share an author; passing the vectors shows consistency of one reading, not independent confirmation (see `packages/bridge-contract/README.md`).
- Proof of possession in the crate is an HMAC stand-in for a signature. Production use should use Ed25519 (already a `termenbox-core` dependency); that is an upstream change with a new vector.
- The case study (D.2) needs real logs from step 5; none exist yet.
