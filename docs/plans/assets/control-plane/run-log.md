# Run log

One row per finished step (procedure `run-a-plan-with-the-dispatcher-discipline`, rule 6). The data R97 and R99 start from.

| Step | Tier | Cost | Owner questions | Check | Predicted vs actual size |
|---|---|---|---|---|---|
| 2.6 | default (Sonnet 5.5), one session | not reported by the host | 0 | `cargo test -p termenbox-server conductor_terminal` passes (6 tests) | M predicted; one PR of about 900 lines across core, conductor, node link and device wiring |
| 2.5 (probe and video) | default (Sonnet 5.5), one session | not reported by the host | 0 | `pnpm check` and the Home and fleet tests pass; the `fleet-needs-you-template` case renders on phone and desktop | S predicted; about 90 lines across the probe page, the recorder's probe start and the case |
| 2.5 (wiring) | default (Sonnet 5.5), one session | not reported by the host | 0 | `pnpm check` and the Home, hub tests pass | S predicted; about 40 lines across hub, Home and wire types; terminal picker left open |
