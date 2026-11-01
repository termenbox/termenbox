---
id: no-alert-without-a-visible-status
title: No sound, vibration, push or badge fires unless the status it announces is rendered, and no needs-you status renders without its alert
status: active
governed-by: 0184-alerts-fire-only-from-a-rendered-attention-edge
grounded-in: [0180-agent-status-is-one-per-session-machine-and-the-workspace-is-a-roll-up, 0184-alerts-fire-only-from-a-rendered-attention-edge, 0071]
derivation-note: Given that the chime, the chip and the push read different state today (fact 0071), a rule on the pair is the only check that catches a sound with no status change and a status with no sound.
tags: [agents, notifications, client, testing]
---

# Guardrail: No alert without a visible status

## Rule
**MUST NOT** play a chime, vibrate, send a push or raise a badge for a session whose `needs_you` or `result` status is not already rendered in the same client frame. **MUST NOT** render a `needs_you` or `result` status that has no Attention item behind it. `info` items are silent by design and are outside this rule.

## Check
Every status e2e scenario runs the cross-surface helper after each step. It asserts that chip, tab strip, sidebar, Home row, title count, badge and inbox agree on the status, that each alert recorded in the step (chime, vibration, notification) follows a rendered `needs_you` or `result` rising edge, and that each such rising edge produced exactly one alert. A scenario without the helper fails CI.
