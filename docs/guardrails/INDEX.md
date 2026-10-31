# Guardrails Index

| ID | Title | Governed By | Grounded In | Status |
|---|---|---|---|---|
| native-wrapper-keeps-web-parity | Native wrappers are optional and the web client stays complete | 0140 | 0002,0003,0010,0140 | active |
| no-paid-services | No Paid Services in Required Stack | 0011,0140 | 0001,0013 | active |
| minimum-browser-support | Minimum Browser Support Policy | 0001,0005 | 0003,0004 | active |
| resource-constraints | Resource Constraints (512MB RAM, 1 vCPU) | 0001 | 0001 | active |
| security-secrets-handling | Secrets Handling (Memory Only, No Logs) | 0006,0009 | 0008,0011 | active |
| internal-network-binding | Internal Network Binding (127.0.0.1 Only) | 0001 | 0001 | active |
| audit-logging-no-secrets | Audit Logging Without Secrets | 0006,0009,0142 | 0008,0011 | active |
| paranoid-mode | Paranoid Mode (Outbound Block) | 0001 | 0001 | active |
| bootstrap-simplicity | 1-2 Click Bootstrap for Users | 0013 | 0001,0011 | active |
| ci-only-builds | All Builds Run in CI Only | 0010,0011 | 0010,0011 | active |
| connection-hints-are-signed-and-never-identity | Candidate bundles are signed by the server key, and no address or intermediary decides who the server is | 0123 | 0057,0051 | active |
| lan-addresses-never-reach-the-directory | Private (LAN) addresses never reach the public directory or telemetry | 0123 | 0056,0051 | active |
| relays-carry-only-ciphertext | A relay or tunnel carries only end-to-end ciphertext keyed to the server | 0123 | 0057 | active |

<!-- bootstrap-generated: true -->
instruction-file-within-cap,"The instruction file must stay within 150 lines","practice-packs, harness, guardrail",active
procedure-delivery-rule,"Detailed procedures live in skills loaded on demand","practice-packs, harness, guardrail",active
noisy-work-rule,"Noisy work is delegated only when the cost model says the handoff is cheaper","practice-packs, harness, guardrail",active
cache-hygiene-rule,"No mid-task model, effort or instruction-file change; switches are confirmed","practice-packs, harness, guardrail",active
compaction-timing-rule,"Context is compacted at task boundaries","practice-packs, harness, guardrail",active
watches-use-the-wrapper,"Watches start through go-getter watch, --until done for CI runs and long commands","practice-packs, harness, guardrail",active
delegations-are-routed,"Delegations to a role are routed by expected total step cost","practice-packs, model-routing, guardrail",active
reviewer-read-only,"The reviewer must not modify files","practice-packs, agent-roles, guardrail",active
parallel-tasks-use-worktrees,"Parallel tasks must each use their own worktree","practice-packs, agent-roles, guardrail",active
max-concurrent-agents-rule,"No more than 3 agents run at once","practice-packs, agent-roles, guardrail",active
human-escalation-rule,"Agents must hand over to a human when blocked, before irreversible actions and on a guardrail denial","practice-packs, agent-roles, guardrail",active
handoff-has-brief-result-evidence,"Handoffs must carry a brief, a result and evidence","practice-packs, agent-roles, guardrail",active
verification-gate-blocks-done,"A step is not done while its verification checks fail","practice-packs, verification, guardrail",active
branch-names-follow-the-pattern,"Branch names must match the project pattern","practice-packs, git-hooks, guardrail",active
commit-subjects-are-conventional,"Commit subjects must follow Conventional Commits","practice-packs, git-hooks, guardrail",active
changes-land-as-one-squashed-pull-request,"A change lands as one squashed pull request with green CI","practice-packs, git-hooks, guardrail",active
no-commits-on-the-default-branch,"Work must not happen on the default branch","practice-packs, git-hooks, guardrail",active
tracker-writes-follow-the-answer,"Writes to the plan tracker follow the project's answer","practice-packs, hooks, guardrail",active
host-mode-explicit-consent,"Host mode needs explicit consent and is audited","security, deployment",active
directory-request-budget,"The directory stays within the free request budget at 10^5 servers","directory, scale, cost",active
github-token-stays-in-browser,"The GitHub token never goes to a server","security, auth",active
releases-are-signed,"Releases, bundled tools and the installer are signed and verified","security, supply-chain",active
host-changes-are-journaled,"Host changes are journaled and revertible","safety, deployment",active
soft-keyboard-never-hides-input,"The soft keyboard never hides the key row or terminal input","client, mobile, ios",active
kill-session-needs-confirmation,"Killing a workspace or an agent tab needs confirmation","client, protocol",active
connections-warn-never-drop,"Connections are limited by warning, never by dropping","client, mobile, performance",active
remote-uninstall-is-deliberate-and-scoped,"Remote uninstall is owner-only, confirmed, fresh and scoped","security, deployment, client",active
usage-stays-on-device,"Usage and tier data stay on the device","client, privacy",active
diagnostics-never-include-secrets,"Problem reports never include secrets, session content or addresses","security, privacy, client",active
unprivileged-server-never-holds-root,"An unprivileged server never holds a way to run root commands","security, deployment",active
updates-are-verified-and-reversible,"Native updates are verified, reversible and stoppable","security, build, deployment",active
ui-never-moves-under-the-finger,"The interface never moves a control or switches the session under the user's finger","client, mobile",active
user-settings-are-wrapped-not-replaced,"Termenbox adds to a user's agent settings and never replaces what is there","hooks, security",active
free-features-are-never-capped-for-a-tier,"A free feature is never removed or capped to create a paid tier","tiers, product",active
paid-data-is-ciphertext-only,"Paid-tier data is stored as ciphertext only","tiers, privacy, security",active
notifications-carry-no-agent-text,"A notification or push payload never carries anything the agent wrote","notifications, privacy, security",active
ux-acceptance-targets,"Every UX screen meets the acceptance targets for taps, glanceability, speed and device checks","client, mobile, ux",active
a-phone-check-names-its-platform,"A phone check names the platform it ran on, and an emulator is never reported as a phone","client, mobile, android",active
| errors-fit-one-screen | An error state fits one phone screen with one cause and one action | 0127 | ux-acceptance-targets,connections-warn-never-drop | active |
| pairing-code-guesses-are-bounded | The short pairing code is stretched and short-lived, and its guesses are budgeted | 0128 | 0021,0022,0117,0123 | active |
| manifest-url-resolution | Asset URLs come from the verified manifest and stay on the project's releases | 0132 | 0063,0076,0130 | active |
| ux-hard-rules-block-the-pr-the-rest-is-advisory | The UX job fails a pull request on objective rule breaks only; trends, diffs and heuristic findings are advisory | 0135 | ux-acceptance-targets,a-phone-check-names-its-platform | active |
| protocol-core-stays-acp-v1 | The protocol core stays ACP v1; extensions are namespaced, negotiated and ignorable | 0136 | 0111,0136 | active |
| control-never-waits-behind-bulk | Control messages never wait behind bulk traffic | 0139 | 0096,0139 | active |
| relay-credentials-are-owner-scoped-minted-after-auth-and-capped | Relay and signaling resources belong to the owner's account, credentials are minted only after authentication, and relayed bytes are capped | 0141 | 0140,0058 | active |
| a-fault-in-one-session-never-stops-the-server | A fault in one session never stops the server, and a death always leaves a record | 0142,0143 | 0142,0143 | active |
| listdir-is-owner-only-home-confined-and-directories-only | Directory listing for the folder picker is owner-only, confined to the home directory, and returns directories only | 0147 | 0140,0058,audit-logging-no-secrets | active |
| ux-scenarios-assert-a-state-before-they-capture-it | A UX scenario asserts the state it claims to show before it records or screenshots it | 0135 | 0135,0056,0148 | active |
| work-happens-in-pool-worktrees | Work happens only in a pool worktree, never in the main checkout | 0150 | 0150,0027,0028 | draft |
| media-commands-come-only-from-people-with-write-access | A render, an approval or a regeneration is accepted only from a person with write access to the repository | 0158 | 0158,0067,no-paid-services | active |
| nothing-is-published-without-a-matching-approval-record | A video or still enters docs/media only from a render a developer ticked, and its render.json hash is the ticked one | 0158 | 0158,0066,ux-scenarios-assert-a-state-before-they-capture-it | active |
