# Decisions Index

| ID | Title | Track | Status | Accepted By | Expires |
|---|---|---|---|---|---|
| 0001 | WebTransport with WebSocket Fallback | product | deferred | architect | 2027-10-09 |
| 0002 | Svelte 5 for Client Framework | product | active | architect | - |
| 0003 | Full Frame Deltas with Rope/Sequence CRDT for SSP | product | deferred | architect | - |
| 0004 | Binary Protobuf-like Wire Protocol with Varint Encoding | process | active | architect | - |
| 0005 | iOS Safari Mitigation via Background Fetch, Push, and Service Worker | product | deferred | architect | 2027-10-09 |
| 0006 | SSH Agent Forwarding for Authentication | product | deferred | architect | - |
| 0007 | PTY Wrapper with Shell Hooks for Agent Integration | product | deferred | architect | - |
| 0008 | xterm.js with WebGL Addon for Terminal Rendering | product | active | architect | - |
| 0009 | ChaCha20-Poly1305 for Packet Encryption | process | deferred | architect | - |
| 0010 | Monorepo with Cargo Workspace and pnpm Workspace | process | active | architect | - |
| 0011 | MIT License for All Components | product | active | architect | - |
| 0012 | vte Crate for Terminal Emulation in WASM | process | deferred | architect | - |
| 0013 | User Self-Hosted Server with Hosted Client | product | active | architect | - |

<!-- bootstrap-generated: true -->
0014-instruction-file-cap,"The always-loaded instruction file is capped at 150 lines","practice-packs, harness, cost",active
0015-procedure-delivery,"Detailed procedures live in skills loaded on demand","practice-packs, harness, cost",active
0016-noisy-work-placement,"Noisy work is delegated only when the cost model says the handoff is cheaper","practice-packs, harness, cost",active
0017-cache-hygiene,"No mid-task model, effort or instruction-file change; switches are confirmed","practice-packs, harness, cost",active
0018-compaction-timing,"Context is compacted at task boundaries","practice-packs, harness, cost",active
0019-watcher-noise,"Watchers are quiet","practice-packs, harness, cost",active
0020-routing-classes,"Routing distinguishes explore, plan, implement, review and debug","practice-packs, model-routing, cost",active
0021-routing-start-tiers,"Classes start at balanced tiers and the lowest effort they allow","practice-packs, model-routing, cost",active
0022-routing-max-escalations,"A failed step escalates at most 2 times, then a human decides","practice-packs, model-routing, cost",active
0023-routing-spend-cap,"Headless runs are capped at 5 USD where the host has a cap","practice-packs, model-routing, cost",active
0024-routing-cache-ttl,"The prompt cache lives one hour in the main session and five minutes in subagents","practice-packs, model-routing, cost",active
0025-agent-roster,"Work is delegated to planner, explorer, implementer, reviewer and tester agents","practice-packs, agent-roles, harness",active
0026-reviewer-access,"The reviewer reports findings and changes nothing","practice-packs, agent-roles, harness",active
0027-parallel-isolation,"Each parallel task runs in its own git worktree","practice-packs, agent-roles, harness",active
0028-worktree-location,"Task worktrees live next to the repository","practice-packs, agent-roles, harness",active
0029-max-concurrent-agents,"At most 3 agents run at once","practice-packs, agent-roles, harness",active
0030-human-escalation,"Agents hand over to a human when blocked, before irreversible or outward-facing actions, and on a guardrail denial","practice-packs, agent-roles, harness",active
0031-handoff-contract,"Handoffs are a brief in, then a result and evidence out","practice-packs, agent-roles, harness",active
0032-verification-checks,"Implement and debug steps pass the project's detected check commands","practice-packs, verification, harness",active
0033-verification-test-command,"The verification test command is ""cargo test""","practice-packs, verification, harness",active
0034-verification-lint-command,"The verification lint command is ""cargo clippy""","practice-packs, verification, harness",active
0035-verification-typecheck-command,"The verification typecheck command is ""cargo check""","practice-packs, verification, harness",active
0036-verification-format-command,"The verification format command is ""cargo fmt --check""","practice-packs, verification, harness",active
0037-verification-static-analysis-command,"The verification static-analysis command is """"","practice-packs, verification, harness",active
0038-verification-build-command,"The verification build command is ""cargo build""","practice-packs, verification, harness",active
0039-verification-unit-command,"The verification unit command is """"","practice-packs, verification, harness",active
0040-verification-integration-command,"The verification integration command is """"","practice-packs, verification, harness",active
0041-verification-e2e-command,"The verification e2e command is """"","practice-packs, verification, harness",active
0043-verification-review-bar,"Review steps pass a reviewer-agent verdict","practice-packs, verification, harness",active
0044-verification-gate,"""Done"" is blocked while the verification checks fail","practice-packs, verification, harness",active
0045-branch-names,"Branches are named ^(feat|fix|docs|chore|ci|test|refactor|perf|build|style|revert)/[a-z0-9][a-z0-9._-]*$","practice-packs, git-hooks, guardrail",active
0046-change-unit,"One change is one branch and one pull request","practice-packs, git-hooks, guardrail",active
0047-default-branch,"The default branch takes no direct work","practice-packs, git-hooks, guardrail",active
0048-refs-trailers,"Refs trailers are encouraged","practice-packs, git-hooks, guardrail",active
0049-plans-in-github,"Plan steps live in GitHub Issues","practice-packs, roadmap",active
0050-tracker-writes,"go-getter may write to the tracker without asking","practice-packs, roadmap",active
0051-mvp-evolve-baseline,"The hybrid MVP is the baseline; v1-only designs are deferred","architecture, roadmap",active
0052-both-hosting-modes,"Hosted client and self-hosted client are both supported","deployment, roadmap",active
0053-single-owner-servers,"A server belongs to one GitHub user","security, auth, roadmap",active
0054-session-backends,"The server owns sessions through a pluggable backend","architecture, sessions, agents",active
0056-automatic-connectivity,"Servers reach the client automatically through a signed directory and quick tunnels","deployment, networking, security, roadmap",active
0057-webrtc-direct-transport,"The browser reaches the server over a direct WebRTC data channel","transport, networking, security, roadmap",active
0058-run-modes,"A server runs sessions in an isolated container or on the host, chosen at install","deployment, security, sessions, roadmap",active
0059-environment-profile-and-bundled-tools,"The environment profile lives on the server and bundled tools update themselves","deployment, sessions, agents, roadmap",active
0060-directory-scale-r2-cdn,"The directory serves at least 10^5 servers on free tiers, with public records on R2 behind the CDN","directory, scale, networking, roadmap",active
0062-device-key-credentials,"Browsers authenticate to servers with a device key and a signed certificate, never the GitHub token","security, auth, roadmap",active
0063-signed-releases-and-opt-in-updates,"Releases are signed, the installer pins digests, and host-mode updates are opt-in","security, supply-chain, deployment, roadmap",active
0064-directory-abuse-controls,"The directory resists abuse and clients keep working when it is exhausted","directory, security, scale, roadmap",active
0065-host-mode-step-up,"Root host sessions need a passkey step-up and devices can be revoked","security, auth, deployment, roadmap",active
0066-web-baseline-and-privacy,"The hosted client ships a strict CSP, a disclosure policy, a privacy notice and data deletion","security, privacy, web, roadmap",active
0067-change-journal-and-revert,"Every host change is journaled, previewed and revertible","safety, deployment, roadmap",active
0068-user-data-in-r2-and-live-sync,"The owner's server list and settings are encrypted objects read from the CDN, with live sync through servers","directory, scale, sync, roadmap",active
0069-ui-foundation-tailwind-shadcn,"The client UI is built with Tailwind v4 and shadcn-svelte","client, build, mobile",active
0071-context-aware-key-row,"The key row adapts to the detected multiplexer, with a sticky Prefix key","client, terminal, mobile, protocol",active
0072-remote-uninstall,"Remove server can uninstall the server in two stages, with host cleanup opt-in","deployment, security, client, protocol",active
0073-usage-tiers-on-device,"Usage tiers and a thank-you badge are computed on the device and never leave it","client, mobile, privacy",active
0074-problem-reports-via-github-issues,"Problems are reported by the user through a prefilled GitHub issue","client, privacy, repo",active
0075-native-host-mode,"Native host mode runs the server without Docker, as a systemd service","deployment, server, security",active
0076-native-releases-and-updater,"Native releases carry a signed manifest and a root-owned updater applies them with rollback","build, security, deployment",active
0077-root-actions-by-request-file,"An unprivileged server asks for root actions through a request file, never through sudo","security, deployment, protocol",active
0078-install-for-the-current-user,"A host install can run sessions as the user who ran the installer","deployment, security, client",active
0079-agent-hooks-are-opt-in,"Agent hooks are installed only on the owner's explicit request, and the layer works without them","agents, hooks, security, roadmap",active
0080-connector-credentials-from-a-root-owned-file,"Connector credentials come from a root-owned file read at start and are held in memory","security, secrets, notifications, roadmap",active
0081-server-name-defaults-to-the-hostname,"A server's display name defaults to the host's short hostname, and a name the owner types wins","server, client, deployment",active
0082-navigation-tabs-sheet-and-auto-connect,"Navigation is a tab strip and a session sheet over servers that connect by themselves","client, mobile, agents",active
0083-cache-expiry-estimate-and-statusline-forwarder,"Prompt-cache expiry is an estimate, made exact by a wrapped status line forwarder","agents, hooks, privacy",active
0084-tab-switch-shows-a-server-snapshot-first,"A tab switch shows a server snapshot at once, then attaches live","client, mobile, performance, protocol",active
0085-tabs-speak-for-one-window-and-detach-differs-from-close,"A tab speaks for one tmux window, Detach differs from Close, and windows come from tmux control mode","client, mobile, agents, tmux, protocol",active
0086-a-problem-report-says-what-the-app-did-and-how-the-page-was-viewed,"A problem report says what the app did and how the page was viewed","client, privacy, mobile, ios",active
0087-the-terminal-is-built-once-and-never-answers-the-page,"The terminal is built once per connection, takes input at 16px and answers no colour queries","client, ios, rendering, terminal, mobile",active
0089-paid-tiers-sell-only-what-costs-us-to-run,"Paid tiers sell only what costs us to run; the free product is never capped","tiers, billing, roadmap",active
0090-pro-is-managed-relay-history-push-and-fleet-view,"Pro is managed relay, cloud history, reliable push with inbox, and fleet view, all client-encrypted","tiers, privacy, crypto, roadmap",active
0091-team-and-enterprise-are-a-hosted-control-plane-over-mit-code,"Team and Enterprise sell a hosted control plane; the code stays MIT","tiers, license, security, roadmap",active
0092-billing-starts-with-github-sponsors,"Billing starts with GitHub Sponsors; a merchant of record arrives with Team","billing, tiers, auth",active
0093-paid-tiers-are-built-after-the-free-gate,"Paid tiers are designed now and built only after the free gate","tiers, roadmap",active
0094-session-guests-get-a-scoped-expiring-grant-not-ownership,"A session guest gets a scoped, expiring grant tied to a GitHub id, never ownership","security, auth, sharing, roadmap",active
0095-agent-status-is-activity-plus-attention,"Agent status is Activity plus Attention, with the current task, and notifications fire on Attention","agents, notifications, pwa, protocol, roadmap",active
0096-terminal-and-status-use-two-data-channels,"Terminal data and agent status travel on two data channels of one connection","transport, protocol, agents, roadmap",active
0097-herdr-is-removed,"herdr is removed as a session manager and as a status source","agents, sessions, architecture, roadmap",active
0098-verification-coverage-command,"The verification coverage command is """"","practice-packs, verification, harness",draft
0099-verification-coverage-bar,"Coverage must meet what the coverage command enforces","practice-packs, verification, harness",draft
0100-home-is-a-decision-first-board,"Home is a decision-first board: what needs the user first, the fleet below","client, mobile, agents, roadmap",active
0101-navigation-bottom-bar-on-phone-workspace-on-desktop,"Phone navigation is a four-destination bottom bar; desktop is a three-pane workspace with a command palette","client, mobile, desktop, roadmap",active
0103-session-view-uses-capability-tiers-and-agent-profiles,"The in-place session view uses three capability tiers and per-agent screen profiles, for every detected agent","agents, client, protocol, roadmap",active
0104-decision-cards-carry-context-and-escalate,"Decision cards carry context, and unanswered decisions escalate by a ladder with a deep-link push","agents, notifications, client, roadmap",active
0105-reports-are-fleet-board-timeline-usage-and-health,"Reporting is a live fleet board, an activity timeline with a daily digest, usage and cost, and server health; on-server views are free","client, tiers, agents, roadmap",active
0106-hook-offer-is-contextual-and-not-a-nag,"Hooks are offered once, inline, in context, and never nagged","agents, hooks, client, roadmap",active
0107-design-spike-precedes-the-build,"A design spike picks the visual direction before screens are built; Android is first-class and honestly labelled","client, mobile, design, roadmap",active
0108-verification-machine-profile,"Verification checks run only in CI (ci-only machine profile)","practice-packs, verification, harness, ci",active
0109-state-first-and-the-terminal-is-first-class,"The product aims for users never needing the terminal, and the terminal is still a first-class, super-convenient surface","client, mobile, agents, terminal, roadmap",active
0110-skins-are-themeable-through-tokens,"The UI is skinnable: components are styled only through design tokens, and several skins ship and are user-selectable","client, design, theming, roadmap",active
0111-acp-sessions-use-one-event-model-and-per-agent-quirk-profiles,"ACP sessions (tier T0) map into the existing agent event model, with per-agent quirk profiles as data","agents, protocol, architecture, roadmap",active
0112-browser-floor-ios-16-4-chrome-111,"The minimum browsers are iOS Safari 16.4 and Chrome Android 111, so Tailwind v4 stays","client, browser-support, design, roadmap",active
0113-host-me-needs-no-root,"The install for the current user needs no root; anything that does is optional and printed, not run","deployment, security, server",active
0114-server-builds-and-tests-run-in-a-pinned-container,"The Rust server and the browser end-to-end tests are built and run in a pinned container, not from a host install","build, repo",draft
0115-several-accounts-on-a-machine-are-one-server-instance-per-os-user,"Several accounts on one machine are served by one server instance per OS user, never by one multi-account server","security, auth, deployment, server",active
0116-an-account-is-a-provider-and-an-id,"An account is a provider and an id, so a second OAuth provider can be added without confusing two users","security, auth, protocol",active
0117-a-claim-link-is-previewed-and-consumed-only-on-confirm,"A claim link is previewed first and consumed only when the user confirms, and the name is editable there","security, auth, client, protocol",active
0118-user-installs-use-a-reserved-udp-range-and-say-what-needs-root-first,"User installs bind a port from a reserved UDP range and say what needs root before changing anything","deployment, network, security, client",active
0119-native-installs-bundle-a-pinned-static-tmux,"Native installs carry a pinned, statically linked tmux, so no install needs tmux from the distribution","deployment, server, security, build",active
0120-termenbox-runs-its-own-tmux-server-with-its-own-config,"Termenbox runs its own tmux server on a private socket with a config it owns, apart from the user's tmux","server, sessions, deployment, compatibility",active
0121-the-tmux-server-is-always-the-private-one-unless-the-owner-opts-out,"Sessions always live on the private tmux server; the default one is used only when the owner opts in","server, sessions, deployment, compatibility",active
0122-agent-state-comes-from-one-turn-and-obligation-reducer,"Agent state is derived by one reducer from open turns and open obligations, fed alike by hooks and ACP; the screen only closes what it no longer shows","agents, server, hooks, protocol",draft
0123-connections-are-a-raced-set-of-signed-candidate-paths,"A server is one identity reached over a raced set of signed candidate paths, so every topology connects without a mode choice","transport, network, security, deployment, roadmap, lan",draft
0124-connection-telemetry-is-opt-out-sampled-and-enum-only,"The hosted client reports connection outcomes as sampled, enum-only events through an IP-stripping proxy, opt-out","privacy, network, client, performance, roadmap",draft
0125-the-roadmap-has-no-priority-tiers,"The roadmap has no priority tiers; order comes from WSJF chains, dependencies and the paid-lane rule","roadmap, repo",active
0129-features-land-on-develop-and-master-moves-only-for-a-release,"Features land on develop, the default branch; master moves only by fast-forward for a release","repo, release, ci, git-hooks",active
0130-releases-have-three-channels-each-with-its-own-signing-key,"Releases have three channels, a manifest names its channel, and each channel has its own signing key","release, updater, security, ci",active
| 0126-the-installer-always-prints-one-pairing-link-qr-and-code | The installer prints one pairing link, QR and short code on every run, carrying the claim and the signed candidates | product | active | Sergey Strebulaev | - |
| 0127-add-server-is-a-four-step-wizard-with-a-standalone-scan | Add Server is a four-step wizard with no fork up front, and Scan QR is a separate path outside it | product | active | Sergey Strebulaev | - |
| 0128-a-short-pairing-code-is-a-rate-limited-pake-secret | A short pairing code is a mailbox key and a PAKE password, bounded by its life, a stretch and a guess budget | product | active | Sergey Strebulaev | - |
| 0129-features-land-on-develop-and-master-moves-only-for-a-release | Features land on develop, the default branch; master moves only by fast-forward for a release | process | active | Sergey Strebulaev | - |
| 0130-releases-have-three-channels-each-with-its-own-signing-key | Releases have three channels, a manifest names its channel, and each channel has its own signing key | product | active | Sergey Strebulaev | - |
| 0131-nightly-installs-notify-and-do-not-update-themselves-by-default | Each server has an update policy, and a nightly install notifies instead of updating itself by default | product | active | Sergey Strebulaev | - |
| 0132-the-signed-manifest-names-the-assets-not-the-release-tag | The signed manifest names where each asset is; an address and a tag never do | product | active | Sergey Strebulaev | - |
| 0133-each-channel-has-its-own-release-workflow-and-the-aliases-only-move-forward | Each channel has its own release workflow and environment, rolling aliases only move forward, and the image tags follow the channel | product | active | Sergey Strebulaev | - |
| 0134-each-channel-has-its-own-pages-deployment-on-its-own-origin | Each release channel has its own Cloudflare Pages deployment on its own origin, deployed by its release workflow, and its installer trusts only its keys | product | draft | - | - |
| 0135-ux-assessment-and-feature-videos-share-one-ci-scenario-set | UX assessment and feature videos are rendered from one set of scripted scenarios in a CI job; hard rules block, the rest is advisory | process | active | Sergey Strebulaev | - |
| 0137-agents-start-in-a-terminal-or-agent-view-chosen-per-agent | An agent starts in a Terminal view or an Agent view (ACP), chosen per agent; ACP sessions stay server-owned and a restart resumes them | product | active | Sergey Strebulaev | - |
| 0136-a-workspace-exposes-its-agent-windows-over-acp-v1-plus-an-extension-registry | A workspace exposes its agent windows over one protocol, ACP v1 plus an extension registry, behind replaceable ports | product | active | Sergey Strebulaev | - |
| 0138-a-workspace-holds-windows-and-a-session-is-an-agent-conversation | A workspace is the sandbox that holds windows; a session is one agent conversation (an ACP sessionId) | product | active | Sergey Strebulaev | - |
| 0139-control-bytes-and-events-travel-on-separate-channels-and-events-replay-from-a-sequence-log | Control, terminal bytes and agent events travel on separate channels, and events replay from a sequence log | product | active | Sergey Strebulaev | - |
| 0140-native-wrappers-are-optional-and-the-pwa-stays-first-class | Native wrappers are optional enhancements and the PWA stays the reference client | product | active | Sergey Strebulaev | - |
| 0141-assisted-connection-runs-on-the-owners-cloudflare-account | A server that cannot be reached directly is helped by free public STUN, a signaling Worker and a TURN key that live in the owner's Cloudflare account, never in the project's | product | active | Sergey Strebulaev | - |
| 0142-the-server-keeps-its-own-jsonl-logs-and-a-crash-record | The server keeps its own JSONL logs and a crash record | process | active | Sergey Strebulaev | - |
| 0143-a-fault-in-one-acp-session-never-stops-the-server | A fault in one ACP session never stops the server, and a crashing server restarts with backoff and a safe mode | product | active | Sergey Strebulaev | - |
| 0144-diagnostics-bundles-are-user-triggered-and-incidents-are-tracked-in-github-and-postmortems | Diagnostics bundles are user-triggered, and incidents are tracked in GitHub issues and postmortems | process | active | Sergey Strebulaev | - |
| 0145-logging-never-waits-for-the-disk | Logging never waits for the disk | process | draft | - | - |
| 0146-a-workspace-id-is-minted-by-the-server-and-the-name-is-only-a-label | A workspace id is minted by the server, and the name the owner types is only a label | product | active | Sergey Strebulaev | - |
| 0147-remote-folders-are-browsed-through-an-owner-only-home-confined-listdir | Remote folders are browsed through a new owner-only, directories-only list_dir request confined to the server user's home | product | active | Sergey Strebulaev | - |
| 0148-the-new-workspace-dialog-shows-recents-and-puts-the-rest-in-layers-inside-it | The New workspace dialog shows three recent agents and a readonly folder with recent chips, and puts the full lists in layers inside the dialog | product | active | Sergey Strebulaev | - |
| 0149-a-workspace-is-a-server-record-and-each-tab-is-a-tmux-window | A workspace is a server record and each tab is a tmux window | product | active | Sergey Strebulaev | - |
| 0150-all-work-happens-in-a-pool-worktree | All work happens in a worktree from a fixed pool, never in the main checkout | process | draft | - | - |
| 0152-a-dormant-workspace-resumes-its-agents-when-reopened | A dormant workspace resumes its agents when it is reopened | product | draft | - | - |
| 0153-the-tab-strip-follows-tmuxs-current-window-on-a-control-mode-server | The tab strip follows tmux's current window on a control-mode server | product | active | Sergey Strebulaev | - |
| 0154-a-new-release-waits-a-grace-period-for-the-owner-then-installs-or-is-skipped | A new release waits a grace period for the owner, then installs or is skipped as the host configured | product | active | Sergey Strebulaev | - |
| 0155-a-container-install-updates-through-a-host-timer-that-pulls-the-digest-the-signed-manifest-names | A container install updates through a host timer that pulls the digest the signed manifest names | product | active | Sergey Strebulaev | - |
| 0156-search-covers-every-known-agent-session-from-the-agents-own-files | Search covers every agent session termenbox knows, read from the agents' own files and scanned on demand | product | active | Sergey Strebulaev | - |
| 0157-agent-session-files-are-read-through-a-session-log-port-with-one-adapter-per-agent | Agent session files are read through a SessionLog port with one adapter per agent, and AcpView reads one unified log | process | active | Sergey Strebulaev | - |
| 0158-documentation-media-comes-from-one-recorder-is-picked-by-a-developer-and-lands-in-docs-media-through-a-pull-request | Documentation videos and stills come from one recorder, are picked one by one by a developer on a preview page, and land in docs/media through a pull request | process | active | Sergey Strebulaev | - |
| 0159-the-docs-are-held-in-step-with-the-code-by-ci-checks-and-generated-reference | The README, guide and site are held in step with the code by CI checks and a reference generated from the binary, with no model in the loop | process | active | Sergey Strebulaev | - |
| 0160-the-user-manual-is-a-guided-course-with-one-video-per-chapter-and-the-guide-stays-as-reference | The user manual is a guided course in docs/course with one video per chapter, Start before Master, and the guide pages stay as the reference | product | active | Sergey Strebulaev | - |
| 0161-course-videos-use-a-human-rhythm-model-a-still-poster-and-steps-generated-from-the-case | Course videos use a human rhythm model for their delays, show a still poster that links to the MP4, and take their numbered steps from the recorded case | process | active | Sergey Strebulaev | - |
| 0165-native-wrappers-ship-no-self-updater-until-the-owner-opts-in | The native wrappers ship no self-updater, and nothing publishes a release, until the owner opts in and updates are signature-checked | process | active | Sergey Strebulaev | - |
| 0166-wrappers-inject-an-optional-capability-host-and-the-client-stays-native-free | Wrappers inject an optional-capability host as a global, and back and keep-awake are wrapper-only chrome | process | active | Sergey Strebulaev | - |
