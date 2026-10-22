---
id: bootstrap-simplicity
title: 1-2 Click Bootstrap for Users
status: active
governed-by: 0013
grounded-in: 0001,0011
tags: [product, ux, deployment]
---

# Guardrail: 1-2 Click Bootstrap

## Rule
User onboarding from zero to working terminal **MUST** complete in ≤ 2 user-initiated actions after VPS is provisioned.

## Required Flow
1. **User provisions VPS** (outside our control — DigitalOcean, Hetzner, etc.)
2. **User runs single command** on VPS:
   ```bash
   curl -fsSL https://get.termenbox.dev | bash
   # or: docker compose -f <url> up -d
   ```
3. **User opens `https://app.termenbox.dev`** → Login with GitHub (1 click)
4. **User clicks "Add Server"** → Scans QR code with phone (1 click)
5. **Terminal works** — types `ls`, sees output

## Total User Actions: 2 clicks (Login + Scan QR)

## Prohibited
- Manual config file editing
- Copy-pasting tokens/URLs between terminal and browser
- Port forwarding / firewall rules
- DNS configuration
- Kubernetes / systemd service files
- Any "read the docs" step before it works

## Enforcement
- E2E test: Fresh VPS → working terminal in < 5 min, ≤ 2 clicks
- CI: `grep -r "nano\|vim\|edit\|config\|yaml\|systemd" docker/` → fail
- Docs: Single `README.md` with copy-paste deploy command

## Cost Corollary
- **User pays**: VPS only ($5–10/mo). No Termenbox subscription.
- **Termenbox pays**: Cloudflare Pages free tier. GitHub OAuth free. No per-user cost.
## Scope (0052)
The two-click bound applies to the Termenbox-hosted client mode. The self-hosted client mode (own domain, Pages, OAuth app, Terraform) is exempt but must be documented as a separate path.

## Scope (0056)
The irreducible human step is pasting one client-generated command on the VPS. It must contain everything needed (secret included); the user never copies a URL, a token shown separately, or a link between terminal and browser. Nothing is printed for the user to act on.

## Scope (0126)
The installer now prints a pairing QR, link and code on every run. Printing is optional for the user: the paste path still needs no action on what is printed, and a LAN-only server is added by scanning it.

## Scope (0127, 0128)
The command no longer carries a secret (0056's "secret included" clause is replaced): it is bare, `curl -fsSL <client>/i | sh`, the same on every topology. The user finishes Connect by scanning the printed QR (one action, as in the Required Flow), or, with no camera, by pasting the printed link or, once the mailbox exists, typing the 8-character code. Pasting the link or typing the code is a fallback to the scan, not a second required step; it is the one place a value passes between terminal and browser, and the "copy-pasting tokens/URLs" prohibition above is relaxed to that fallback. The README stays the single entry with the copy-paste command; its reference sections live in `docs/guide/`.
