---
id: 0066
title: Anthropic's terms allow a subscription login in the unmodified Claude Code binary, but point Agent SDK products at API keys
status: active
tags: [agents, acp, legal]
date: 2026-11-01
kind: environmental
governed-by: 0158
last-verified: 2026-11-01
---

# Anthropic's terms allow a subscription login in the unmodified Claude Code binary, but point Agent SDK products at API keys

Read on 2026-11-01 from code.claude.com/docs/en/legal-and-compliance ("Authentication and credential use" and "Can customers offer Claude Code in their products?"). Not legal advice; re-read before relying on it.

- OAuth (Free, Pro, Max, Ultra, Team, Enterprise) is "intended exclusively" for ordinary use of Claude Code and other native Anthropic applications.
- Developers building products or services, "including those using the Agent SDK, should use API key authentication" through Claude Console or a supported cloud provider. Anthropic "does not permit third-party developers to offer Claude.ai login into their own applications, or to route requests through Free, Pro, Max, or Ultra plan credentials on behalf of their users", and developers may not collect, store or intermediate Claude.ai credentials or session tokens.
- The page does not restrict an end user "from signing in to the unmodified Claude Code binary with their own Claude subscription", including where a platform hosts it. The binary must not be modified, and each end user authenticates with their own credentials.
- The page sends edge cases to Anthropic sales.

What it means here: the Terminal view runs the unmodified binary, so a subscription login is within the text. `claude-agent-acp` is built on the Agent SDK, so a subscription login through Agent view falls in the restricted category or a grey area, even though termenbox is self-hosted and never touches the credential. Decision 0158 therefore makes Agent view the Claude default only on API-key or cloud credentials. The owner may still ask Anthropic sales for a written answer (R74 holds that gate).
