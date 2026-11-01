# Attention control plane: conductor, operator session, cluster cards, contracts, deputy and the 10^4 levers, built dogfood-first

Decisions: 0179 (the dispatcher), 0169 (the product and its metric), 0170 (conductor), 0171 (operator session), 0172 (layers and mission contracts), 0173 (deputy ladder), 0174 (cluster stream and cards), 0175 (escalation, team mode planned), 0176 (the four 10^4 levers), 0177 (Agent view pauses), 0178 (dogfood-first). Fact 0070 (attention arithmetic), fact 0069 (Anthropic terms). Guardrails `deputy-treats-agent-text-as-data-and-picks-from-enumerated-answers`, `irreversible-classes-never-leave-propose-without-a-checkpoint`, `the-deputy-is-optional-capped-and-owner-supplied`, `operator-actions-carry-an-actor-id-apart-from-the-owner`, `conductor-links-carry-attention-metadata-only`, `control-plane-steps-are-dogfooded-before-they-are-done`, `auto-answered-permissions-are-scoped-visible-and-audited` (extended), `a-cheaper-tier-needs-a-verifier-for-the-outcome`, `ui-features-ship-templates-tests-and-media-before-wiring`. Procedures `run-a-plan-with-the-dispatcher-discipline`, `dogfood-a-control-plane-slice`, `add-a-decision-class-to-the-deputy`, `write-a-mission-contract-template`, `build-an-agent-view-slice-template-first`. Roadmap steps R87 (meter), R88 (conductor), R89 (operator session), R90 (clusters), R92 (escalation), R91 (contracts), R93 (deputy), R94 (reversible), R95 (learn), R96 (budget), R97 (hierarchy), R98 (team mode), R99 (task telemetry), R100 (outcome specs and decomposition), R101 (router), R102 (pre-flight and combining). Related: R81 and R85 (kept), R82 to R84 and R86 (paused, decision 0177), R74, R75, R62.

## Cold start

Termenbox is a web terminal for remote AI coding agents: Rust server `packages/server`, shared wire types `packages/core/src/protocol.rs` (mirrored by hand in `packages/client/src/lib/protocol.ts`), Svelte 5 client `packages/client/src`. Read `CLAUDE.md` first. Work only in a pool worktree (`sh scripts/pool.sh acquire feat/control-plane-<step>`), one squashed PR per step off `develop`, commit subjects `type(scope): summary` with `Why:`/`What:` body, no Co-Authored-By. No compiler on some machines: prefix commands with `scripts/dev.sh`. Constraints: 512 MB RAM and 1 vCPU for termenbox's own processes, secrets never logged, no paid service in the required stack, the web client stays complete.

**The reframe (decision 0169).** Termenbox becomes an attention control plane: one person keeps 100 to 1,000 agents productive from any device. The metric is attention leverage, human seconds per agent-hour. Asks are removed in five layers (0172): mission contract, rule, deputy, cluster card, human. A fleet of nodes is run under one conductor server (0170); the operator's own place follows them between devices (0171); the human works in one stream of cluster cards in a sliding window (0174); unanswered obligations escalate on a time-boxed ladder with a safe default (0175). Four more levers take it toward 10^4 (0176).

**Dogfood-first (decision 0178, read the guardrail).** The owner uses each slice on their own machines before it counts as done. M1 comes first and is thin: the meter plus a conductor with two or three of the owner's own nodes and the existing Needs-you list across them. A step is `[x]` only when its Check passes and `docs/plans/assets/control-plane/dogfood.md` has the owner's dated note for it (procedure `dogfood-a-control-plane-slice`). Agents and CI never deploy to or edit the owner's own `termenbox.service`: they hand over a build, the owner installs. The owner's confirmation is the only thing that can close a "Dogfood" step.

**UI rule.** Every UI step follows `build-an-agent-view-slice-template-first`: fixtures, templates, tests, styling, video and manual, owner review, then wiring. It needs R81 step 0.5 (the probe page and media pipeline in `docs/plans/agent-view-programme.md`); if that is not done, do it first. Design rules are in `docs/plans/assets/ux/DESIGN-PRINCIPLES.md` (44 pt targets, at most 7 controls per row, state never by colour alone, chrome at most 20% of the height).

**The dispatcher (decision 0179).** Above the five layers sits a dispatcher: outcome specs, decomposition, decision pre-flight, routing by verified quality and combining, to minimise human seconds, cost and unverified risk together. Telemetry (step 1.4) starts in M1 so the baseline week already holds cost data; the rest is slice 10 and acts only after shadow mode.

**Milestones.** M1 = slices 0 to 2 (meter and conductor, dogfooded). M2 = slices 3 to 5 (operator session, clusters, escalation). M3 = slice 6 (rules and contracts; Agent view R82 to R84 and R86 resumes after it, decision 0177). M4 = slice 7 (deputy, in shadow mode first). M5 = slice 8 (levers). M6 = slice 9 (team mode). M7 = slice 10 (dispatcher).

**Where today's code is.** Attention and Activity state: `packages/server/src/events.rs`, `reducer.rs` (decision 0122), `workspaces.rs`, `push.rs`, `turn.rs`; ACP in `acp.rs`, `acp_stream.rs`; wire types `ClientMessage` and `ServerMessage` in `packages/core/src/protocol.rs`. Client: `Home.svelte`, `lib/cards.ts`, `NeedsYouBanner.svelte`, `InboxSheet.svelte`, `DecisionPanel.svelte`, `QuickReply.svelte`, `lib/acpStore.ts`, `lib/protocol.ts`. The candidate-path connection model is decision 0123 (`candidates.rs`, `connection.rs`).

## Slice 0: scaffolding and the map

### 0.1 Dogfood log and its format — [x]‖
Class: implement · Needs: none · Check: `test -f docs/plans/assets/control-plane/dogfood.md && grep -q "Template" docs/plans/assets/control-plane/dogfood.md`
Context: Guardrail `control-plane-steps-are-dogfooded-before-they-are-done` requires a dated owner note per step in `docs/plans/assets/control-plane/dogfood.md`. The file does not exist yet.
Do: Create the file with a short header and a "Template" section: `## <step id> <date>` then lines `Days used:`, `Before (s per agent-hour):`, `After:`, `Hurt:` (three items), `Keep / change / revert:`. Add nothing else. Do not fill any note: only the owner does.
Done-when: the file exists and contains the template section.

### 0.2 Map today's attention path — [x]‖
Class: explore · Needs: none · Check: `test -f docs/plans/assets/control-plane/map.md && grep -c "events.rs" docs/plans/assets/control-plane/map.md`
Context: Every later step extends the attention path; the cold session needs it written down. Decisions 0095 and 0122 describe intent; the code is the truth.
Do: Write `docs/plans/assets/control-plane/map.md`: where an obligation is raised, cleared and acknowledged (`packages/server/src/events.rs`, `reducer.rs`, `workspaces.rs`, `acp_stream.rs`); which `ClientMessage` and `ServerMessage` variants carry Attention, answers and acks; what is per server and what is per device; where push is triggered (`push.rs`); where the client builds Needs-you (`Home.svelte`, `lib/cards.ts`, `InboxSheet.svelte`). Give file and line for each. List what a multi-node merge would have to rename or key (ids per node).
Done-when: the file names each of those modules with line references and a section "Ids that need a node key".

## Slice 1: the meter (R87)

### 1.1 Server meter: layers, dwell and answer latency — [ ] (code and Check done, awaits the 1.3 dogfood note)
Class: implement · Needs: 0.2 · Check: `scripts/dev.sh cargo test -p termenbox-server attention_meter`
Context: Decision 0169: attention leverage is human seconds per agent-hour. Data stays on the server, metadata only, bounded (`resource-constraints`, decision 0105 for the pattern). No prompt or agent text is stored.
Do: Add `packages/server/src/attention_meter.rs`: for every obligation record `raised_at`, `resolved_at`, the layer that resolved it (`human`, `rule`, later `contract`, `deputy`), the answering device (never a name), and for a human the dwell (card foreground time reported by the client) and answer latency. Keep a ring of the last 30 days as daily buckets in the server's state directory, atomically written. Compute `human_seconds` and `agent_hours` (sum of working time per tab from the reducer) per day and a 7-day figure. Add `ClientMessage::CardSeen { obligation, ms }` and `ServerMessage::AttentionStats { days }` in `packages/core/src/protocol.rs`, mirrored in `packages/client/src/lib/protocol.ts` with a round-trip test. Test: bucket rollover, bounded size, a rule-resolved obligation adds zero human seconds, no text stored.
Done-when: the tests pass, and a test asserts the stored file contains no field of agent text.

### 1.2 Client: the meter line on Home — [ ] (code and Check done, awaits the 1.3 dogfood note)
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/AttentionMeter.test.ts'`
Context: Decision 0169. The owner must see their own number every day for dogfood notes. Keep it one line, no chart yet. Chrome share limit DR12.
Do: Add `packages/client/src/AttentionMeter.svelte` (props only: days, window) showing "Attention: N s per agent-hour (7 days), M% of asks answered without you", shown on `Home.svelte` under the fleet summary; send `CardSeen` from the Needs-you card dwell. Test names, the empty state (no agents), and that it never shows agent text.
Done-when: the test passes and `Home.test.ts` still passes.

### 1.4 Task telemetry: tokens, cost, model, verifier result, decision points (R99) — [ ] (code and Check done, awaits the 1.3 dogfood note; open: ACP reports no model so its turns count under `unknown`, agent-reported ACP costs are not yet folded into the day cost, and the verifier outcome has no producer until 10.1)
Class: implement · Needs: 1.1 · Check: `scripts/dev.sh cargo test -p termenbox-server task_telemetry`
Context: Decision 0179: the dispatcher cannot be tuned without per-task data, and the baseline week (1.3) should already hold it. Token use arrives in ACP `usage` updates (`packages/client/src/lib/acpStream.ts`, `packages/server/src/acp_stream.rs`) and through the Claude status-line forwarder (R19). Metadata only; no agent text (`resource-constraints`, decision 0105).
Do: Extend `packages/server/src/attention_meter.rs` (or add `task_telemetry.rs` beside it): per turn or task record agent, model id, tokens in and out, cache reads, estimated cost from a small price table the owner can edit, verifier outcome when one exists (pass, fail, none), and the number of obligations raised. Roll into the same 30-day buckets. Add the fields to `AttentionStats` in `packages/core/src/protocol.rs` and the client mirror. Show "$ per verified outcome" on the meter line when data exists. Test bounded size, no text stored, and a turn with no usage data.
Done-when: the tests pass and a test asserts the stored file holds no agent text.

### 1.3 Dogfood: the baseline week — [ ]
Class: review · Needs: 1.2, 1.4 · Check: `grep -q "^## 1.3" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178. Without a baseline no later slice can show it helped. The owner installs the build; agents do not touch the owner's service.
Do: Hand the owner the build with the meter and the task telemetry (1.4) on. The owner runs it for 7 days of normal work and writes the note `## 1.3 <date>` with the 7-day figure and what hurt. Record the number in `docs/plans/assets/control-plane/baseline.md`.
Done-when: the note and `baseline.md` exist with the human-seconds figure and the dollars per verified outcome (or "no verifier").

## Slice 2: the conductor (R88), M1

### 2.1 Spike: conductor protocol, enrolment and failure behaviour — [x]
Class: plan · Needs: 0.2 · Check: `test -f docs/plans/assets/control-plane/conductor.md`
Context: Decision 0170 and guardrail `conductor-links-carry-attention-metadata-only`. Nodes dial out to the conductor over the existing signed identity (decision 0123; `candidates.rs`, `connection.rs`, `signal.rs`). Ids are per server today (see the map from 0.2).
Do: Write `docs/plans/assets/control-plane/conductor.md`: the node-to-conductor messages (hello with node key and name, obligation raised, cleared, acked, answered, usage, incident, node health), the enrolment flow (a pairing link from the conductor, one tap on the node's owner device, using the pairing in `--print-pairing`), how ids are keyed `(node, workspace, tab, obligation)`, reconnect and bounded-queue behaviour while a node is away, what the conductor shows when a node is silent, how a device opens a terminal on a node (proxy through the conductor, or a signed path), and the budget for 50 idle agents per node in bytes per minute. State the standby design for later step 2.8.
Done-when: the file exists and has a section for each item above.

### 2.2 Conductor wire types — [x] (in `packages/core/src/conductor.rs`, a module of its own rather than `protocol.rs`, which is already 2,000 lines)
Class: implement · Needs: 2.1 · Check: `scripts/dev.sh cargo test -p termenbox-core conductor`
Context: Guardrails `conductor-links-carry-attention-metadata-only` and `operator-actions-carry-an-actor-id-apart-from-the-owner`. Types are shared in `packages/core/src/protocol.rs` and mirrored by hand.
Do: Add `NodeMessage` and `ConductorMessage` enums to `packages/core/src/protocol.rs` per the spike, every answer and acknowledgement carrying `actor` and `owner`, agent text length-capped; mirror them in `packages/client/src/lib/protocol.ts`; add round-trip tests with old-client compatibility (unknown variants ignored).
Done-when: the tests pass, including one that fails on an answer without `actor`.

### 2.3 Node side: the outbound link — [x] (answers, acks and terminal opens arriving from the conductor are ignored here; they land in 2.4 and 2.6. Candidates are tried in order, not raced. A clear is reported as `by: fact` until the answer path of 2.4 can say otherwise)
Class: implement · Needs: 2.2 · Check: `scripts/dev.sh cargo test -p termenbox-server conductor_link`
Context: A node behind NAT must reach the conductor with no open port. Decision 0170.
Do: Add `packages/server/src/conductor_link.rs` and a `--conductor <pairing>` option in `main.rs`: one outbound connection with backoff, sending the node's attention changes and health, a bounded queue while disconnected (drop oldest with a counted notice), and a switch that disables it. Off unless configured. Test with a fake conductor: reconnect, queue bound, no terminal bytes sent.
Done-when: the tests pass and a test asserts no terminal bytes cross the link.

### 2.4 Conductor side: accept nodes and merge one stream — [x] (`conductor.rs`: enrolment by key with a 15-minute one-time secret and the owner's tap, ordered merge keyed `(node, epoch, id)`, 256 KiB per node, silent node, answer and ack routing with `idem`, `serve` over a `Link`, hooked into `connection.rs` behind `--accept-nodes`. The surfaces that call it are step 2.4b)
Class: implement · Needs: 2.2 · Check: `scripts/dev.sh cargo test -p termenbox-server conductor`
Context: Decision 0170. The merged stream is keyed `(node, workspace, tab, obligation)`. The server advertises `conductor: true` in its hello so the client can switch to the merged view (compatible with old clients).
Do: Add `packages/server/src/conductor.rs`: enrol nodes by key, merge their Attention into one ordered stream, bounded per node by bytes and age, forward answers back to the owning node, mark a silent node "silent" after a threshold. Test with three simulated nodes: ordering, a silent node, an answer reaching the right node, memory bound.
Done-when: the tests pass, including 3 nodes and 50 agents each under the byte budget from the spike.

### 2.4b Surfaces of the conductor: enrol, tap, hello flag, apply answers on the node — [ ]
Class: implement · Needs: 2.4 · Check: `scripts/dev.sh cargo test -p termenbox-server conductor_surface`
Context: Step 2.4 built the state and the accept path; nothing yet mints the secret, shows the pending node, tells clients the server is a conductor, or applies a routed answer on the node, so a real fleet cannot be enrolled. Decision 0170, `conductor.md` sections 1 and 2.
Do: (1) Mint the enrolment secret inside the running server (a request through the `requests_dir` or control channel, never a log line) and print the link and QR with `--conductor-enrol`, same shape as `--print-pairing` plus the secret outside the signed part. (2) Add the wire messages for the owner's device: the node list with pending nodes, `approve_node`, `remove_node`, and the merged stream, with `conductor: true` in `auth_ok` (old clients ignore it) and the mirror in `packages/client/src/lib/protocol.ts`; every operator action carries `actor` and `owner`. (3) On the node, apply a routed `Ack` (`EventHub::ack`) and `Answer` (`answer_tab`, the prompt matched by the obligation's id, the same refusal when stale), once per `idem`, and report `Cleared { by: answer }` instead of `fact`. Test with a conductor and a node in one process: enrol, tap, answer reaches the node and clears there, a stale answer is refused, an old client still connects.
Done-when: the tests pass and a node enrolled with the printed link appears pending, then live after the tap.

### 2.5 Client: Needs-you across nodes — template, tests, review — [ ]
Class: implement · Needs: 2.4b, 1.2 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm check && pnpm vitest run src/Home.test.ts src/lib/fleetFixtures.test.ts'`
Context: Decision 0100 (Home) with 0170. Follow `build-an-agent-view-slice-template-first`: fixtures first (`packages/client/src/lib/fleetFixtures.ts`: 3 nodes, 40 obligations, one silent node), then props-only templates, tests, styling, video and manual, owner review, then wiring.
Do: Show the merged Needs-you list with a node chip per item, a silent-node notice, and the meter line. Wire it to the conductor stream only after the owner accepts the video. Terminal open on an item goes through the conductor.
Done-when: the check passes, the video and manual exist, and `docs/plans/assets/agent-view/reviews.md` (or `docs/plans/assets/control-plane/reviews.md`) records the owner's acceptance.

### 2.6 Open a node's terminal through the conductor — [ ]
Class: implement · Needs: 2.4b · Check: `scripts/dev.sh cargo test -p termenbox-server conductor_terminal`
Context: Terminal bytes cross the link only on an explicit open (guardrail `conductor-links-carry-attention-metadata-only`).
Do: Add open and close of a terminal on a node through the conductor, stopping on close; test that bytes flow only between open and close and that a closed device leaves nothing open.
Done-when: the test passes.

### 2.7 Dogfood M1: the owner's own fleet — [ ]
Class: review · Needs: 2.5, 2.6, 1.3 · Check: `grep -q "^## 2.7" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178. The owner enrols two or three of their own machines to a conductor they pin, behind the switch, and works from one phone connection. This is the first milestone.
Do: Hand over the build and the install command. The owner runs it for 7 days and writes `## 2.7 <date>` with the 7-day meter reading beside the 1.3 baseline and what hurt. Turn each pain into a step or an edit to 2.8, 3.x or 4.x before they are wired.
Done-when: the note exists and the pains are listed as steps or edits in this plan.

### 2.8 Warm standby conductor — [ ]
Class: implement · Needs: 2.7 · Check: `scripts/dev.sh cargo test -p termenbox-server conductor_standby`
Context: Decision 0170 names the conductor a single point of failure in the first version; the spike (2.1) states the standby design.
Do: Implement the standby: a second server replicates the conductor's operator state and rules, nodes know both, and the standby takes over after a missed-heartbeat window with a visible incident. Test failover and no double answers.
Done-when: the test passes and an answer made during failover is applied once.

## Slice 3: the operator session (R89)

### 3.1 Server operator session: cursor, drafts, lease — [ ]
Class: implement · Needs: 2.7 · Check: `scripts/dev.sh cargo test -p termenbox-server operator`
Context: Decision 0171 and guardrail `operator-actions-carry-an-actor-id-apart-from-the-owner`. The session lives on the conductor.
Do: Add `packages/server/src/operator.rs`: per actor a session with stream cursor, open card, drafts per card, window, mode, last-seen cursors; a lease that makes one device active and others passive, "continue here" to take it, answers idempotent by card id (first wins; others get "answered on <device>"). Wire messages carry `actor` and `owner`. Tests: two devices answering the same card, lease expiry, resume after all devices were off.
Done-when: the tests pass, including double answer and resume.

### 3.2 Client: resume, continue here and passive state — template first — [ ]
Class: implement · Needs: 3.1 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/lib/operatorSession.test.ts src/OperatorHandoff.test.ts'`
Context: Decisions 0171 and 0167. Resume must land on the same card in under one second on the owner's phone and desktop.
Do: Template (props only) and tests for: attach and resume banner, "continue here", passive read-only state with "take over", the "answered on phone" state. Fixtures first, video and manual, owner review, then wire to `lib/operatorSession.ts`.
Done-when: tests pass and the owner has accepted the video.

### 3.3 Dogfood: phone to desktop and back — [ ]
Class: review · Needs: 3.2 · Check: `grep -q "^## 3.3" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178; the point of the slice is that the owner does not lose their place.
Do: Owner uses it for 5 days moving between phone and desktop; note the number of times the place was wrong and the meter reading.
Done-when: the note exists.

## Slice 4: cluster cards (R90)

### 4.1 Study: how much would clustering save on the owner's real obligations — [ ]
Class: explore · Needs: 1.3 · Check: `test -f docs/plans/assets/control-plane/cluster-study.md`
Context: Decision 0174 bets that equivalent obligations are common. Use the metadata from the meter (class features, never text) on the owner's baseline week; do not read the owner's sessions.
Do: Add a throwaway analysis (script in the scratch area, not committed) over the stored obligation metadata and write `cluster-study.md`: the proposed class key features, how many distinct keys the week had, the share of obligations in a cluster of 2 or more, and 5 mis-cluster risks.
Done-when: the file states the distinct-key count and the clustered share.

### 4.2 Class key and cluster builder — [ ]
Class: implement · Needs: 4.1 · Check: `scripts/dev.sh cargo test -p termenbox-server cluster`
Context: Decisions 0173 and 0174 and the procedure `add-a-decision-class-to-the-deputy` (step 1: the class key from stable features only).
Do: Add `packages/server/src/cluster.rs`: `class_key(obligation)` from agent, tool kind, workspace and a normalised command or change shape; a builder that groups open obligations into clusters with the shared fact, an outlier list and a split action. Tests: equal obligations share a key, different ones do not, an outlier is listed, a compound shell command is never merged with a simple one.
Done-when: the tests pass.

### 4.3 Cluster card and sliding window — template, tests, review — [ ]
Class: implement · Needs: 4.2, 2.5 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/ClusterCard.test.ts src/lib/window.test.ts && node e2e/agentview.mjs --audit'`
Context: Decision 0174: the card is a brief (ask, deciding facts, recommendation and confidence, risk line, time-box), evidence one tap away; the window shows the current card, the next two and a count, and prefetches the next evidence. The stream obeys `ui-never-moves-under-the-finger`. Follow the template-first procedure.
Do: Fixtures (`clusterFixtures.ts`: singleton, 37-member cluster with outliers, high-risk, timed-out), templates `ClusterCard.svelte`, `CardWindow.svelte`, `lib/window.ts`; tests; styling; scenario, video and manual; owner review.
Done-when: tests and audit pass and the owner has accepted the video.

### 4.4 Wire cluster answers — [ ]
Class: implement · Needs: 4.3 · Check: `scripts/dev.sh cargo test -p termenbox-server cluster_answer`
Context: One tap answers all members, idempotent by card id (decision 0171); each member answer is audited with `actor` and carries the cluster id; "make this a rule" opens the rule creation of R85.
Do: Add the answer path in `conductor.rs` and the client wiring; handle a member that changed between display and tap (answered elsewhere, or gone): answer the rest and show the count. Test the race.
Done-when: the test passes and an answer reaches every still-open member exactly once.

### 4.5 Dogfood: clusters on the owner's fleet — [ ]
Class: review · Needs: 4.4 · Check: `grep -q "^## 4.5" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178. The mis-cluster rate and the meter reading are the evidence.
Do: Owner uses the stream for 7 days; note the meter before and after, the splits they had to make, and cards that were too long.
Done-when: the note exists; the pains are plan edits.

## Slice 5: escalation and incidents (R92)

### 5.1 Urgency, time-box, safe default, incidents — [ ]
Class: implement · Needs: 3.1, 2.4 · Check: `scripts/dev.sh cargo test -p termenbox-server escalation`
Context: Decision 0175. The safe default comes from the contract (R91), else from the class history, else "park the agent". Push uses `push.rs` and `notifications-carry-no-agent-text`.
Do: Add `packages/server/src/escalation.rs`: obligations carry `urgency` and `deadline`; the ladder (card, push, louder push and second device, safe default plus an incident entry); incidents for contract breach (later), stuck agent past its threshold, silent node, deputy demotion (later), timed-out default; incidents dedupe by cause. Tests with a fake clock for each rung and for dedupe.
Done-when: the tests pass, including that a timeout applies the safe default once.

### 5.2 Incident cards in the stream — template first — [ ]
Class: implement · Needs: 5.1, 4.3 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/IncidentCard.test.ts'`
Context: Decision 0175: incidents show what the system did on its own, in the same stream.
Do: Template and tests, then video, review, wiring. An incident card says what happened, what the system did, what is left to the human, in plain words.
Done-when: tests pass and the owner has accepted the video.

### 5.3 Dogfood M2 — [ ]
Class: review · Needs: 5.2, 4.5, 3.3 · Check: `grep -q "^## 5.3" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178. M2 closes slices 3 to 5. The owner sleeps through a night with the ladder on.
Do: Owner runs 7 days, including nights; note wake-ups caused, wrongly applied defaults and the meter reading.
Done-when: the note exists and says keep, change or revert.

## Slice 6: rules and mission contracts (R85 rescoped, R91), M3

### 6.1 Re-scope R85 to stand alone — [ ]
Class: plan · Needs: none · Check: `python3 docs/plans/roadmap/check.py`
Context: Decision 0177 keeps R85 but pauses R82 and R84; R85's UI steps (4.1 to 4.8 in `docs/plans/agent-view-programme.md`) assume the inspector's Rules tab. `docs/plans/roadmap/R85.md` has `Preds: R82, R84`.
Do: R85's `Preds` was already set to `R81` when this plan was written; edit steps 4.x of the programme so the Rules sheet opens from the Home stream and from a card, and the inspector Rules tab is a later item under R84. Keep server steps unchanged. Run the check.
Done-when: `check.py` passes and `grep -n "^Preds" docs/plans/roadmap/R85.md` shows only R81.

### 6.2 Mission contract model and generator — [ ]
Class: implement · Needs: 6.1 · Check: `scripts/dev.sh cargo test -p termenbox-server contract`
Context: Decision 0172; procedure `write-a-mission-contract-template`; guardrails `auto-answered-permissions-are-scoped-visible-and-audited` and `irreversible-classes-never-leave-propose-without-a-checkpoint`. Rules matching is R85's parsed-argument matcher in `acp.rs`/`acp_stream.rs` (reuse, do not duplicate).
Do: Add `packages/server/src/contract.rs`: a contract (workspaces, paths, commands, tool kinds, network hosts, spend, wall time, files changed, done criteria, safe default, urgency), compiled to rule entries and agent modes; answers tagged `contract:<id>`; a breach raises an incident (5.1). Tests: compound commands never match, the irreversible floor holds, a breach is an incident, a contract never widens an agent's sandbox.
Done-when: the tests pass.

### 6.3 Launch with a contract — template, tests, review — [ ]
Class: implement · Needs: 6.2, 4.3 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/ContractSheet.test.ts'`
Context: Decision 0172; the owner reviews the confirmation wording (0165 pattern) and the dry run.
Do: Template `ContractSheet.svelte` (props only) in New Tab / New Workspace: scope summary in plain words, limits, safe default, a dry-run preview from the last 50 asks, save as template. Fixtures, tests, video, review, wiring.
Done-when: tests pass and the owner has accepted the video.

### 6.4 Suggest a contract from history — [ ]
Class: implement · Needs: 6.3 · Check: `scripts/dev.sh cargo test -p termenbox-server contract_suggest`
Context: Decision 0172: contracts are suggested from past approvals, never invented.
Do: From repeated approvals of the same kind of work in a workspace, build a candidate contract with its dry run; never create it without the owner's confirm. Test that a candidate covers only what was approved.
Done-when: the test passes.

### 6.5 Dogfood M3: contracts on real work, then resume Agent view — [ ]
Class: review · Needs: 6.4, 5.3 · Check: `grep -q "^## 6.5" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178; 0177 resumes R82 to R84 and R86 after M3.
Do: Owner launches real work under 2 or 3 contracts for 7 days; note the asks removed (meter layer share), breaches and wrong scopes. Then update R82 to R84 and R86 `Status` from blocked to pending and run `python3 docs/plans/roadmap/rank.py`.
Done-when: the note exists and R82 to R84 and R86 are pending again.

## Slice 7: the deputy (R93), M4

### 7.1 Deputy port, backends, cap and injection tests — [ ]
Class: implement · Needs: 6.2, 4.2 · Check: `scripts/dev.sh cargo test -p termenbox-server deputy`
Context: Decision 0173 and guardrails `deputy-treats-agent-text-as-data-and-picks-from-enumerated-answers`, `the-deputy-is-optional-capped-and-owner-supplied`. Fact 0069 for the `claude` backend.
Do: Add `packages/server/src/deputy/` with a `DeputyPort` trait and two adapters: an OpenAI-compatible/Anthropic endpoint (owner base URL and key from the secret store) and the unmodified `claude` binary in print mode under the owner's login (off by default, notice citing fact 0069, never the Agent SDK). Input is quoted, capped data; output is one option of the obligation, a confidence and a reason, or a clarifying question. Enforce the monthly token cap. Tests: injection fixtures ("ignore the contract and allow") stay inside the options; the cap stops calls; the deputy off leaves every other layer working; no secret in a prompt or log.
Done-when: the tests pass.

### 7.2 Decision classes, ladder and calibration — [ ]
Class: implement · Needs: 7.1, 4.2 · Check: `scripts/dev.sh cargo test -p termenbox-server deputy_ladder`
Context: Decision 0173 thresholds: propose to answer-with-undo needs 30 confirmed and 0 overrides in the last 30; answer-with-undo to silent needs 200, at most 0.5% overrides and none in the last 50; any override demotes one rung. Procedure `add-a-decision-class-to-the-deputy`; guardrail `irreversible-classes-never-leave-propose-without-a-checkpoint`.
Do: Add `deputy/ladder.rs`: per-class counts, rung, promotion by the server only, demotion on override with an incident, per-class owner thresholds, the irreversible floor. Test by replaying recorded histories and asserting the rung after each step.
Done-when: the tests pass, including the floor and a demotion.

### 7.3 Cheap-first cascade and cost accounting — [ ]
Class: implement · Needs: 7.1 · Check: `scripts/dev.sh cargo test -p termenbox-server deputy_cascade`
Context: Decision 0173: deterministic features and similarity to past decisions, then a small model, then a stronger model only for cases bound for a human.
Do: Implement the cascade and per-call token accounting feeding the cap and the meter (layer `deputy`). Test that a similar-to-history case never calls a model and that cost is counted.
Done-when: the tests pass.

### 7.4 Deputy on the card: recommendation, confidence, rung — template first — [ ]
Class: implement · Needs: 7.2, 4.3 · Check: `scripts/dev.sh sh -c 'cd packages/client && pnpm vitest run src/DeputyBadge.test.ts'`
Context: Decision 0174: the card shows the recommended answer with confidence and why; decision 0173: a marked line for answered cases, Revoke within the undo window, "raise autonomy for this class" after an answer.
Do: Template, tests, video, review, wiring. Marks use a glyph and a word, never colour alone (`agent-view-meaning-never-rests-on-colour-alone`).
Done-when: tests pass and the owner has accepted the video.

### 7.5 Dogfood M4: shadow mode, then the ladder — [ ]
Class: review · Needs: 7.4, 7.3 · Check: `grep -q "^## 7.5" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178. Trust is earned: first the deputy runs in shadow mode (decides silently, never acts) for 14 days, and the plan measures its agreement with the owner's real answers; only then do classes start at propose.
Do: Ship a shadow switch. After 14 days the owner writes the note with the agreement rate by class and the meter reading; classes with high agreement start at propose (they still need the 30 confirmed of 0173).
Done-when: the note exists with an agreement rate.

## Slice 8: the 10^4 levers (R94 to R97), M5

### 8.1 Checkpoints: a verified restorable state per agent (R94) — [ ]
Class: implement · Needs: 6.2 · Check: `scripts/dev.sh cargo test -p termenbox-server checkpoint`
Context: Decision 0176 lever 1 and guardrail `irreversible-classes-never-leave-propose-without-a-checkpoint`. Agent workspaces are directories on the node; a git worktree or a snapshot gives undo.
Do: Add `packages/server/src/checkpoint.rs`: create a checkpoint when a contract-bound run starts and before a risky class runs, verify it restores (a test restore into a scratch dir), expose undo, and expire by bytes and age. The server reports `checkpoint: verified` to the ladder. Tests: restore, expiry, a failed verify leaves the class at propose.
Done-when: the tests pass.

### 8.2 Non-blocking asks: act, report, undo — [ ]
Class: implement · Needs: 8.1, 7.2 · Check: `scripts/dev.sh cargo test -p termenbox-server nonblocking_ask`
Context: Decision 0176: with a verified checkpoint a class may be answered with undo; the agent continues and the case sits in the stream as a review item.
Do: Allow answer-with-undo for classes with a verified checkpoint, show a "review, undo" card that restores the checkpoint, and measure the share of asks that stopped blocking in the meter.
Done-when: the tests pass, including undo restoring the files.

### 8.3 Learn from answers: standing instructions (R95) — [ ]
Class: implement · Needs: 6.2, 4.4 · Check: `scripts/dev.sh cargo test -p termenbox-server standing_instruction`
Context: Decision 0176 lever 2. An answer and its optional one-line reason become a standing instruction or a contract clause, shown to the owner before it applies. The agent receives it through its normal context channel for the agent (instruction file or the first prompt), per its profile.
Do: Add `standing_instruction.rs`: proposal from a repeated answer, owner confirmation, per-workspace store, delivery per agent profile, and a recurrence counter (how often the same class asks again). Test that nothing applies without confirmation and that recurrence is counted.
Done-when: the tests pass.

### 8.4 Attention budget (R96) — [ ]
Class: implement · Needs: 1.3, 6.2 · Check: `scripts/dev.sh cargo test -p termenbox-server attention_budget`
Context: Decision 0176 lever 4. Budget: minutes a day and interrupts an hour; the system steers by proposing contract tightening, class promotions and quieter urgency, never by hiding obligations.
Do: Add the budget settings and a controller that reads the meter, raises an incident when the budget is breached, and proposes (never applies) the steering actions. Test that a breach proposes and that nothing is hidden.
Done-when: the tests pass.

### 8.5 Spike: lead agents and sampled audit (R97) — [ ]
Class: plan · Needs: 7.5 · Check: `test -f docs/plans/assets/control-plane/hierarchy.md`
Context: Decision 0176 lever 3. Lead agents run sub-fleets of about 20 and report exceptions and digests; outcomes are audited by a sample sized for a confidence bound; machine verification (tests, CI, a reviewer agent) is the evidence.
Do: Write `hierarchy.md`: the lead role contract (what it may decide, what it must escalate, its budget), the digest format, the sampling rule (sample size for a 95% bound on defect rate; defects tighten the class), how a lead's decisions enter the ladder, and the failure modes (a lead hides a problem). Propose the implementation steps and append them to this plan as slice 8b.
Done-when: the file exists and has those sections; slice 8b steps are appended.

### 8.6 Dogfood M5: each lever for its own week — [ ]
Class: review · Needs: 8.2, 8.3, 8.4 · Check: `grep -q "^## 8.6" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178. One lever per week so the meter shows what each did.
Do: Owner enables checkpoints and non-blocking asks, then standing instructions, then the budget, one per week; note the meter reading and the recurrence rate for each.
Done-when: the note exists with one reading per lever.

## Slice 9: team mode (R98), M6

### 9.1 Spike: roles, zones and quorum — [ ]
Class: plan · Needs: 5.3 · Check: `test -f docs/plans/assets/control-plane/team.md`
Context: Decision 0175 plans zones of responsibility and quorum; guardrail `operator-actions-carry-an-actor-id-apart-from-the-owner`; decisions 0053 and 0062 (one owner, device keys).
Do: Write `team.md`: the actor model (person, device keys, role), zones (workspaces, classes, nodes, off-hours cover), quorum (N distinct approvers for a class marked high-risk, with a time-box and the safe default), how the conductor routes a card to a zone, how audit separates actors, the security review (a delegate cannot widen a contract or raise the ladder), and the migration from single owner. Append the implementation steps to this plan as slice 9b.
Done-when: the file exists with those sections and slice 9b is appended.

## Slice 10: the dispatcher (R100 to R102, M7)

### 10.1 Outcome spec — [ ]
Class: implement · Needs: 6.2, 1.4 · Check: `scripts/dev.sh cargo test -p termenbox-server dispatcher_spec`
Context: Decision 0179 and guardrail `a-cheaper-tier-needs-a-verifier-for-the-outcome`. The contract (`packages/server/src/contract.rs`, step 6.2) already carries done criteria; verifiers must be machine-run.
Do: Add `packages/server/src/dispatcher/spec.rs`: an outcome spec with verifiers of three kinds (command that must pass, diff bound, reviewer check by a different model through the deputy port), run by the server on the node after a task, with the result recorded in the telemetry (1.4). Tests: a failing command fails the task, a missing verifier is reported as "none", a reviewer check never uses the same model as the author.
Done-when: the tests pass.

### 10.2 Decomposition and decision pre-flight — [ ]
Class: implement · Needs: 10.1, 4.2 · Check: `scripts/dev.sh cargo test -p termenbox-server dispatcher_split dispatcher_preflight`
Context: Decision 0179. The class key is `cluster.rs::class_key` (4.2). Predictions start crude and are compared with actuals.
Do: Add `dispatcher/split.rs` (propose a split of a mission into tasks with a predicted cost vector: tokens, quality risk, decision points, reversibility; from history of the same class, else a conservative default) and `dispatcher/preflight.rs` (list the decisions the tasks are predicted to raise, ask them once as one card, turn the answers into contract clauses; anything unanswered falls through to the ladder). Record prediction versus actual per task. Tests: a split of a fixture mission, preflight reduces the asks in a replayed history, a wrong prediction still reaches the ladder.
Done-when: the tests pass, including a replayed history in which pre-flight cuts the number of asks.

### 10.3 Router in shadow mode — [ ]
Class: implement · Needs: 10.1, 7.2, 1.4 · Check: `scripts/dev.sh cargo test -p termenbox-server dispatcher_router`
Context: Decision 0179; promotion follows decision 0173 and the procedure `add-a-decision-class-to-the-deputy`. The objective weights are owner settings.
Do: Add `dispatcher/router.rs`: for each task recommend agent, model tier and node minimising `w1*human_seconds + w2*cost + w3*(1-verified_quality)` from the class history, within the contract; never recommend below the default without a verifier; never act. Record recommendation versus what ran and the outcome. Test the objective with fixtures, the verifier rule, and that shadow mode changes nothing.
Done-when: the tests pass and shadow mode produces a recommendation log with no side effects.

### 10.4 Combining: co-locate tasks that share context — [ ]
Class: implement · Needs: 10.2 · Check: `scripts/dev.sh cargo test -p termenbox-server dispatcher_combine`
Context: Decision 0179: tasks that share a contract, a decision class or a large context run together for cache reuse and one set of asks. Placement uses conductor node state (2.4).
Do: Add `dispatcher/combine.rs`: group tasks by shared contract, class and context, place a group on one node and session within the node's capacity, and report the cache-read share in the telemetry. Test grouping, capacity limits and that no group crosses a contract boundary.
Done-when: the tests pass.

### 10.5 Router acts, per class, after the owner confirms — [ ]
Class: implement · Needs: 10.3, 10.6 · Check: `scripts/dev.sh cargo test -p termenbox-server dispatcher_router_acts`
Context: Decision 0179 and the guardrail `a-cheaper-tier-needs-a-verifier-for-the-outcome`: shadow mode first, then the owner confirms per class.
Do: Let the router choose the tier and node for a class the owner has confirmed from the shadow log; escalate one tier on a failed verification and record it; one switch turns routing back to the default for everything. Test escalation, the confirmation gate and the off switch.
Done-when: the tests pass, including an unconfirmed class staying on the default.

### 10.6 Dogfood M7a: shadow mode — [ ]
Class: review · Needs: 10.2, 10.3, 10.4 · Check: `grep -q "^## 10.6" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178. The router first only recommends for 14 days; the owner compares it with what ran.
Do: Owner runs 14 days in shadow mode and writes `## 10.6 <date>`: agreement rate, dollars and human seconds per verified outcome against the 1.3 baseline, mis-splits and pre-flight cards that were wrong or too long. Confirm which classes may be routed.
Done-when: the note exists with those figures.

### 10.7 Dogfood M7b: the router acts — [ ]
Class: review · Needs: 10.5 · Check: `grep -q "^## 10.7" docs/plans/assets/control-plane/dogfood.md`
Context: Decision 0178; guardrail `a-cheaper-tier-needs-a-verifier-for-the-outcome`.
Do: Owner runs 14 days with routing on for the confirmed classes and writes `## 10.7 <date>`: dollars and human seconds per verified outcome against 1.3 and 10.6, escalations, quality regressions, and keep, change or revert.
Done-when: the note exists with those figures.

## Rules for the whole plan

- Keep this file's status markers current. When a step lands, set `[x]` only with its dogfood note (guardrail `control-plane-steps-are-dogfooded-before-they-are-done`), then update the roadmap step file and run `python3 docs/plans/roadmap/check.py && python3 docs/plans/roadmap/rank.py`.
- The owner's pains from a dogfood note become plan steps or edits before the next slice is wired.
- If a threshold in decision 0173 changes, edit the decision first, then the code.
