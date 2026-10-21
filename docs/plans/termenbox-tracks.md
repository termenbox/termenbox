# Two-person split of the roadmap (proposal)

Proposed 2026-10-16, not yet applied to `termenbox-roadmap.md` (no `Owner:` field yet). Split by code ownership, balanced by step size (`S` in each step's `Score`): 167 points for Track 1 and 165 for Track 2, plus the shared gate U16 (3). Paid-tier steps (R40 to R48, 48 points) are left for later. Step ids refer to `roadmap/`; R55, R56, R57, R58 and R60 are in `roadmap/archive/` today and would be unfolded back into standalone desktop steps.

| | **Track 1** (core, agents, phone terminal) | **Track 2** (client shell, design system, desktop, cloud and release) |
|---|---|---|
| **Points** | **167** | **165** (plus the shared gate U16, 3) |
| **UX programme** | U03, U06, U07, U07a, U07b, U07c (key row), U08 (decision card), U09, U10, U11, U12, U13, U14 | U02 (design system), U04 (shell), U05 (Home), U15 (desktop workspace) |
| **Desktop fixes on today's client** | none | R55, R56, R57, R58, R60 (unfolded again), R61, R59, R36 |
| **Agents and phone checks** | R19, R21, R03 | none |
| **Server and host** | R23, R24, R29, R30, R33 | R27, R28, R25, R31, R38 |
| **Cloud, release, CI, security** | none | R07, R08, R09, R17, R18, R26, R50, R51, R32, R34 |
| **Other client work** | none | R16, R22, R39 |
| **Connectors** | R10, R11, R14, R15 | none |

Boundaries: Track 1 owns `packages/server`, `packages/core`, the protocol mirror and the phone terminal parts (`KeyRow`, mobile parts of `Terminal.svelte`). Track 2 owns the client shell, `src/lib/ui`, `app.css`, the Pages functions and the workflows. `Terminal.svelte` is Track 1's file; Track 2 changes it through reviewed PRs.

Handoffs: U02 (Track 2) blocks Track 1's client work in U06, U07b and U07c, so it goes first and U06 is best split into a server half (startable now) and a client half; U05 (Track 2) blocks U08 and U10; U08 (Track 1) blocks U15; R08 (Track 2) blocks U12.
