---
id: 0053
title: A server inside a NAT'd VM (WSL2 NAT mode, VirtualBox NAT, Docker Desktop) is reachable from other devices only through port forwarding or a bridged network
status: active
kind: observation
governed-by: 0123-connections-are-a-raced-set-of-signed-candidate-paths
tags: [network, deployment, wsl, lan, topology]
date: 2026-10-22
last-verified: 2026-10-22
---

# A server inside a NAT'd VM needs port forwarding to be reached from other devices

## What is true

A server that runs inside a VM behind the host's own NAT has an address in the VM's subnet (for WSL2 in NAT mode, a Hyper-V virtual switch address). That address is private and is not routable from other devices on the host's LAN. The server advertises it as its `lan` candidate (0123), so the candidate is wrong for every client except software on the host itself.

Reaching it from another device needs one of:

1. **Bridged or mirrored networking**, so the VM shares the host's LAN address. For WSL2 this is `networkingMode=mirrored` in `.wslconfig`, then `wsl --shutdown`, plus a Windows Firewall inbound rule (and, if still blocked, the Hyper-V firewall setting). The installer prints these steps when it sees WSL (`packages/client/public/install.sh`, the `microsoft` check in `/proc/version`).
2. **Port forwarding** from the host's LAN address to the VM, one rule per port. `netsh interface portproxy` forwards TCP only, so it can carry the WebSocket fallback but not the UDP WebRTC path.
3. An **overlay** address (0123 rung 4) installed inside the VM.

The same shape applies to VirtualBox in NAT mode and to Docker Desktop's VM, where a published port is forwarded by the product and the container's own address is internal.

## Not verified

- Whether the WebSocket fallback works from another device on a bare WSL2 host after a `portproxy` rule. The installer says the app "falls back to a slower connection"; no run on a real WSL host is recorded.
- The Windows and WSL versions that support mirrored mode.
- Whether the browser on the Windows host itself reaches the server through localhost forwarding, for UDP as well as TCP.

A check on a real WSL host should be recorded here as a dated result.

## Consequences

- The server's advertised address cannot be trusted to be the one a LAN device can reach; the client's race (0123) finds that out, and the diagnostic should name the cause, not only "server not found".
- No relay is needed once mirrored mode or forwarding is set up; without them, only the host itself and an overlay work.
