# Conductor spike: protocol, enrolment and failure behaviour

Step 2.1 of `docs/plans/attention-control-plane.md`. Governing: decision 0170 (conductor), guardrail `conductor-links-carry-attention-metadata-only`, guardrail `operator-actions-carry-an-actor-id-apart-from-the-owner`. Ids and scopes are from `map.md`. This file is the input to 2.2 (wire types), 2.3 (node link), 2.4 (conductor), 2.6 (terminal) and 2.8 (standby); when a step finds it wrong, edit this file first.

Vocabulary: the **conductor** is one termenbox server the owner pins; a **node** is any other termenbox server enrolled to it; a **device** is a phone or browser of the owner. A conductor is also a node of itself (its own workspaces enter the merged stream as node `local`), so a one-machine owner needs no second server.

## 1. Node-to-conductor messages

Two enums in `packages/core/src/protocol.rs` (step 2.2), JSON like the rest, unknown variants ignored, `v` on `NodeHello`. Every text field is capped (prompt 512 chars, task 160, name 64), control characters stripped, run through the same scrub as the push payload; none carries terminal bytes.

`NodeMessage` (node to conductor):

| Variant | Fields | When |
|---|---|---|
| `Hello` | `v`, `node` (server id), `name`, `epoch`, `version`, `caps`, `enrol` (first time), `sig` (signature over the challenge nonce) | once per connection, after the handshake of section 2 |
| `Snapshot` | `epoch`, `items: [Obligation]`, `workspaces: [WorkspaceState]` | right after `Hello`, and when the conductor asks (`Resync`) |
| `Raised` | `epoch`, `seq`, `obligation` | an Attention item is raised (`events.rs` `raise`) |
| `Cleared` | `epoch`, `seq`, `id`, `by` (`fact`, `ack`, `answer`, `rule`) | cleared by fact, ack or an answer |
| `Acked` | `epoch`, `seq`, `id`, `actor`, `owner` | an ack made on the node's own device |
| `Answered` | `epoch`, `seq`, `id`, `actor`, `owner`, `device`, `option` | an answer made on the node's own device |
| `Usage` | `epoch`, `seq`, `rows: [{workspace, tab, agent, model, tokens_in, tokens_out, cache_read, cost_micro, working_s}]` | a turn ends, coalesced to at most one message a minute, only changed rows |
| `Incident` | `epoch`, `seq`, `cause`, `workspace?`, `detail` (capped) | stuck agent, crash, queue overflow, update failed |
| `Health` | `epoch`, `seq`, `agents`, `working`, `waiting`, `load`, `queue_dropped` | every 30 s as the heartbeat |

`Obligation` = `{ id, workspace, tab?, kind, raised_at, urgency?, class_features, prompt? }`. `class_features` (agent, tool kind, normalised command shape) are the stable features step 4.2 needs, so the conductor never has to read a prompt to cluster. `prompt` is the capped, scrubbed text the card shows; it is the only agent text on the link and it is dropped from a stored record once the obligation clears.

`ConductorMessage` (conductor to node):

| Variant | Fields | When |
|---|---|---|
| `Welcome` | `epoch_ack`, `heartbeat_s`, `rules` (later) | answers `Hello` |
| `Resync` | none | the conductor lost its place: the node sends `Snapshot` |
| `Answer` | `target: {workspace, tab, prompt_id}`, `id`, `actor`, `owner`, `device`, `option`, `idem` | an operator answered a card of this node |
| `Ack` | `id`, `actor`, `owner`, `idem` | an operator acknowledged |
| `OpenTerminal` | `ticket`, `workspace`, `tab`, `cols`, `rows` | section 6 |
| `CloseTerminal` | `ticket` | section 6 |
| `TerminalInput` | `ticket`, `data` (base64) | keystrokes for an open terminal (section 6) |
| `ResizeTerminal` | `ticket`, `cols`, `rows` | section 6 |
| `Disable` | none | the owner removed the node; the node stops dialling |

The node validates an `Answer` exactly as it validates one from a local device (`seq` and prompt match in `answer_tab`, `workspaces.rs`); a stale one is refused with `Cleared { by: answer }` for the winner, so a double answer is harmless. `idem` makes redelivery after a reconnect apply once. `actor` and `owner` are mandatory on `Answer`, `Ack`, `Answered` and `Acked`; a message without them is dropped and counted (the step 2.2 test).

## 2. Enrolment

Reuses what a phone already does (decision 0123, `candidates.rs` `pairing_bundle`, `main.rs` `--print-pairing`), so there is one trust model.

1. **On the conductor:** `termenbox-server --conductor-enrol` (or a button in Settings) prints a link and QR of the same shape as the pairing block: the conductor's signed `Bundle` (its server key, fingerprint and candidate addresses) in the fragment, plus a one-time `enrol` secret outside the signed part, valid 15 minutes, single use, held in memory only (`security-secrets-handling`).
2. **On the node:** `termenbox-server --conductor '<link>'` verifies the bundle signature, pins the conductor's key and fingerprint, then dials the candidates, racing them as clients do, with the node's own server key as identity (built in step 2.3: candidates are tried in order, a race is later work). It does not trust an address that fails the pinned fingerprint.
3. **Handshake:** the conductor sends the existing `Challenge { nonce, server_id }`. The node answers `NodeHello` with its server id, `name`, the `enrol` secret (first time only) and a signature over the nonce with its server key. The conductor checks the secret, records the node key as enrolled, and burns the secret.
4. **Owner tap:** the conductor shows "Add node *name* (key fingerprint)?" to the owner's device; one tap confirms. Until the tap the node is `pending` and its messages are discarded. This is the single human step. (The tap is on the owner device at the conductor, which the owner already holds; the node's own owner device is not needed because the secret proved physical access to the node's terminal.)
5. **Afterwards** the node is identified by its key alone: it reconnects with a signed nonce, no secret. Removing a node deletes its key and sends `Disable` if connected.

The node stores the pinned conductor (key, fingerprint, last good address) in its config directory next to its server key; the file is `0600`. A node with no `--conductor` never dials anything (off by default, section 5 of step 2.3).

## 3. Id keying

Every id that leaves a node is qualified at the conductor as `(node, epoch, workspace, tab, obligation)` per the table in `map.md`:

- `node` is the server id (ed25519 public key, base64url). It is the key; the display `name` is a label and may repeat.
- `epoch` is a random 64-bit value the node picks at process start. The attention counter `a<n>` restarts at 1, so `(node, epoch, id)` is the obligation's identity; a new epoch makes the conductor discard that node's open items it has not seen in the new `Snapshot` (they cannot be answered any more).
- `seq` is a per-epoch counter on every `NodeMessage` after `Hello`. The conductor applies messages in `seq` order, ignores a duplicate, and asks for `Resync` when it sees a gap.
- The merged stream orders by `raised_at`, ties broken by `(node, id)`; the order of an item never changes after it is shown (`ui-never-moves-under-the-finger`).
- A device's wire id for an item is the conductor's own opaque `card` id, mapped to the tuple above in the conductor only. Devices never see a node epoch.

## 4. Reconnect and the bounded queue

- The node dials with exponential backoff, 1 s doubling to 60 s with jitter; it dials all pinned candidates in parallel as the client does.
- While disconnected the node keeps **a bounded queue of 256 messages and 64 KiB**, by whichever is hit first. `Raised`/`Cleared` for the same id collapse (a raise followed by its clear is dropped from the queue); `Usage` coalesces to the latest row set. When full, the oldest `Health`, then `Usage`, then oldest `Raised` is dropped and `queue_dropped` is incremented.
- On reconnect the node sends `Hello` (same `epoch`) then `Snapshot`, then the queue in order. A `Snapshot` replaces the conductor's view of that node, so the queue only needs to carry what the snapshot cannot: answers/acks made on the node and incidents. `queue_dropped` is reported in `Health` and raises an `Incident { cause: "queue overflow" }`.
- The conductor bounds its side per node by **bytes (256 KiB) and age (items cleared for more than 24 h dropped, 7-day incident history)**, dropping oldest with a visible count (guardrail). The per-node cap holds the 200-item `MAX_ATTENTION` of a node with room to spare.

## 5. A silent node

- The node is `live` while a `Health` (or any message) arrived in the last 90 s (three heartbeats). After that it is `silent`. After 10 minutes of silence it raises an incident "node *name* silent" (escalation ladder in step 5.1; until then a notice only).
- A silent node's open items **stay in the stream, marked "node silent since HH:MM"**, never removed and never auto-answered; they cannot be answered (the Answer button reads "node away") until it returns, because there is nobody to apply the answer. Answers a device made while the node was silent are **not** queued as pending; the conductor refuses them with the reason, so the owner never believes a silent node got an answer.
- On return the `Snapshot` reconciles: items gone are cleared with `by: fact`.
- A node removed by the owner is deleted with its history; a node never seen again for 30 days is shown as "gone" with a remove action and not counted in the meter denominator.

## 6. Opening a terminal on a node

Chosen: **proxy through the conductor over the node's already open outbound link**, the same connection, text frames tagged with the `ticket` (`terminal_output` and `terminal_closed` from the node, `terminal_input` and `resize_terminal` from the conductor, bytes in base64). Built in step 2.6: a second binary lane would have needed a second kind of frame on a link that is otherwise all `NodeMessage` and `ConductorMessage`, for traffic that exists only while the owner has a terminal open; a lane is a later change if the 33% base64 overhead hurts. Reasons: a node behind NAT has no inbound path, so a signed direct path would need the conductor to broker hole punching for every open and fails on symmetric NAT; a proxy always works, costs conductor bandwidth only while a terminal is open, and keeps the single connection per device of decision 0170.

Flow: the device sends `OpenTerminal { node, workspace, tab }` to the conductor; the conductor mints a `ticket`, tells the node `OpenTerminal { ticket, ... }`, and from then until `CloseTerminal` (or the device leaving, or the node link dropping) bytes flow in both directions tagged with the ticket. Nothing flows before the open or after the close (step 2.6 test). Terminal bytes are not scrubbed or stored by the conductor; they pass through a fixed-size buffer (16 chunks of 4 KiB, 64 KiB per open terminal), and at most 4 terminals are open per device. The node pump blocks on its bounded queue, so a slow conductor stops the pty; a device that falls behind the buffer loses that terminal (the node is told to close it) rather than growing memory. A connection shows one node terminal at a time: opening another, or attaching a local tab, closes the first. A direct signed path is a later optimisation when the node and device share a LAN (decision 0123 candidates); it is not in this slice.

The conductor's terminal proxy is subject to the per-connection relay accounting that already exists (`signal.rs` `Usage`) only when it goes through the owner's Cloudflare relay; between the owner's own machines it does not.

## 7. Budget: 50 idle agents on one node

Target: **at most 4 KiB per minute in steady state** (heartbeats, no changes), and at most 64 KiB for a `Snapshot` of 200 items. Steady state is idle, so it is dominated by `Health`.

| Item | Rate | Size | Per minute |
|---|---|---|---|
| `Health` | 2 a minute | about 120 B | 240 B |
| `Usage` for changed rows | 0 when idle; 1 a minute worst case with 5 rows | 5 x 90 B + 60 B | 510 B |
| `Raised` + `Cleared` | 1 pair a minute (an active node, not idle) | 600 B + 100 B | 700 B |
| framing (WebSocket, length, ping) | | | 150 B |
| **Total** | | | **about 1.6 KiB** against the 4 KiB limit |

The test in 2.3 and 2.4 drives 50 agents for a simulated hour and asserts the node-to-conductor bytes stay under 4 KiB per minute averaged, and the conductor's memory for three such nodes under 1 MiB. A conductor with 20 nodes sees about 32 KiB a minute inbound, which is trivial against the 512 MB envelope.

## 8. Standby design (for step 2.8)

- A **standby conductor** is another termenbox server enrolled with the same owner. The active conductor streams its operator state (rules, contracts, ladder counts, operator session when slice 3 exists, node registry with keys) to the standby over the same kind of outbound link, as an append-only log with a monotone `lsn`; the standby applies it and holds no live node links.
- Nodes are given both conductors' pinned keys at enrolment (`Welcome` carries the standby's bundle). A node dials the active one; if it cannot connect for **90 s** (three heartbeats) it also dials the standby.
- The standby takes over only if **both** its own link to the active has missed heartbeats for 90 s **and** at least one node reports it cannot reach the active (a node sends `PeerReport` on dialling the standby). This quorum of two views prevents a partition between the two conductors alone promoting the standby. It raises a visible incident "conductor failed over".
- **No double answers:** an answer carries `idem` and the node applies a given `idem` once, whichever conductor delivered it; the conductor that wins is the one that holds an `epoch` lease (`conductor_epoch`, incremented on takeover, sent in `Welcome`); a node ignores a conductor with a lower epoch. A returning old active sees the higher epoch and demotes to standby.
- Devices learn the standby's bundle with the node list and fall back to it as a second candidate when the primary fails, so the owner reconnects without a manual step.

## 9. Open points for later steps

- Whether the conductor's address should be carried by the existing directory record (decision 0056) for a roaming conductor; not needed while the owner's conductor has a stable address or tunnel.
- Whether `Usage` should carry per-turn rows or per-minute sums; the meter (1.1) works on per-day sums, so per-minute is enough.
- The deputy and rules (slices 6 and 7) add `Welcome.rules` and a `RulesUpdate`; they are out of this spike.
