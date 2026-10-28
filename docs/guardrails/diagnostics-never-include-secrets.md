---
id: diagnostics-never-include-secrets
title: Problem reports never include secrets, session content or addresses
status: active
governed-by: 0074,0144
grounded-in: 0074
derivation-note: A report is posted publicly on GitHub, so it must be safe to read by anyone. The server-built diagnostics bundle (0144) is held to the same rule.
tags: [security, privacy, client]
---

# Guardrail: Problem reports never include secrets, session content or addresses

## Rule
**MUST** scrub certificates, claim secrets, tokens, IP addresses and ports from report text and from the server's diagnostics bundle (logs, crash files) and show it to the user before it leaves. **MUST NOT** include terminal output, keystrokes or session names. **MUST** test the scrubber with realistic samples.
