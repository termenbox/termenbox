# Windows support: terminal backend, GUI streaming, secure desktop

Status: draft, not yet a decision or a roadmap item. Nothing here is measured on Windows; every "spike" below
exists to replace an assumption with a number.

## 0. Policy question (answer before any code)

The server is Linux-only today (static musl builds, Docker). Windows is the largest share of the intended users.
Two capabilities decide how large and how risky the Windows work is, and the first is a policy choice, not an
engineering one.

**P1. May a phone type into the Windows secure desktop?**
The secure desktop shows the UAC prompt, the lock screen, the sign-in screen and Ctrl+Alt+Del. Capturing it needs a
SYSTEM service and a SYSTEM helper process; sending input to it makes termenbox a remote path to approve elevation and
to type the Windows password. Pick one:

| Answer | Meaning | Consequence |
|---|---|---|
| A. Never | Secure screens are not captured or driven. The stream shows "a secure prompt is open on the PC". | No service, no SYSTEM code. Phase 3 is dropped. |
| B. View only | The helper captures the secure desktop; input is blocked there. | Needs the service and helper; no password or UAC approval from the phone. |
| C. View and input | Full control, including password entry and UAC approval. | Needs the service, helper and `SendSAS`; highest risk; needs the controls in phase 3. |

Recommendation: **A for the pilot**, and decide between B and C only when phase 3 is scheduled.

**P2. Is the GUI stream in scope for the pilot, or only the terminal?**
The terminal backend (phase 1) is required for any Windows support. The GUI stream (phase 2) is the larger and
riskier piece. Recommendation: ship phase 1 first as its own release.

**P3. Does a Windows workspace have to survive a server restart?**
tmux gives this on Linux. A Windows backend does not get it for free (see phase 1). Recommendation: no for the
pilot, stated plainly in the docs.

**P4. Which Windows editions are in scope?**
Home, Pro and Enterprise all run a user-level server. Windows' own RDP host exists only on Pro and above, which is
why the plan does not depend on it.

Record the answers as a decision (superseding nothing yet) before phase 1 starts.

## 1. Terminal backend on Windows

### Why

Nothing in the terminal path is available on Windows today: tmux is not packaged for it, and the server shells out
to tmux in about 25 files (`backend.rs`, `control.rs`, `tabstatus.rs`, `agentwatch.rs`, ...).

### Options

| Option | Reuses tmux code | Risk |
|---|---|---|
| WSL2 with the Linux build | Fully | WSL must be installed and running; NATed UDP breaks direct reachability and LAN pairing without extra work. |
| psmux (tmux-compatible, Windows native) | Mostly | Control mode and format-string coverage are unproven for the commands we use. |
| In-process backend on `portable-pty` (ConPTY) | No | A new `WorkspaceBackend`; owns scrollback, replay and tab metadata. |
| WezTerm mux | No | No headless attach path; depends on unstable internal crates. Not recommended. |

The commands the tmux backend uses are few (new-session, new-window, send-keys, capture-pane, list-panes,
set-option, select-window, kill-window, rename-window, attach-session, show-options, has-session, display-message),
so a replacement is bounded. `WorkspaceBackend` (DECISION-0054) is already a trait.

### Recommended path

1. Spike psmux for one day against the command list above and the control-mode triggers in `control.rs`. If it
   passes, native Windows reuses most of the backend.
2. If it fails, build an in-process backend behind `WorkspaceBackend`: `portable-pty` for the PTYs, a screen model
   crate (`wezterm-term`, `alacritty_terminal` or `vt100`) for the grid, and the registry for the tab record
   (DECISION-0149). Persistence across client disconnects is easy; across server restarts it is not, and P3
   decides whether a keeper process is needed.
3. Port the Unix-only pieces (see the table below). Run the server as a user-session process, not a service.

### Unix-only code to port

| Piece | Where | Windows answer |
|---|---|---|
| Interface addresses (`getifaddrs`) | `candidates.rs` | `if-addrs` or `network-interface` |
| Disk space (`statvfs`) | `health.rs` | `GetDiskFreeSpaceExW` |
| File locks (`flock`) | `candidates.rs` | `LockFileEx` via `fs2` or `fd-lock` |
| Echo-off password prompt (`termios`) | `cloudflare.rs` | console mode API |
| Child cleanup (`prctl`, `kill(-pid)`) | `acp.rs` | Job Objects |
| File modes and ownership checks | hooks files, `auth.rs` | ACLs on the state directory |
| systemd, loginctl, runuser, `/tmp/tmux-$uid` | `backend.rs`, `main.rs`, installer | Task Scheduler or a Run key; per-user state under `%LOCALAPPDATA%` |
| POSIX shell hooks for Claude Code, Codex | `claude_hooks.rs`, `opencode_hooks.rs` | PowerShell or a small Rust helper; Claude Code expects Git Bash on Windows |

### Done when

A Windows machine runs the server, the phone attaches, reconnects, opens more than one tab and shows agent status,
with the CI matrix building and testing the Windows target. The CI leg runs on a Windows runner (a platform label
on every check, per `a-phone-check-names-its-platform`).

## 2. GUI streaming (windows of the host desktop)

### What "native" can mean

Modern RDP (Windows 8 and later) does not send GDI drawing orders; it encodes dirty regions with H.264 or the
progressive codec and keeps a few cheap commands for fills, copies and cached bitmaps. No remote-display protocol
can send real drawing primitives for modern GPU-composited apps. The target is therefore damage-driven,
hardware-encoded video, which matches RDP's traffic profile.

### Design

- A new window kind `display` beside `terminal` (DECISION-0138, DECISION-0149, R75): one surface per window, input
  events (pointer, key, scroll, text), a resize message, and an opt-in per window.
- Capture: Windows Graphics Capture for a single window (works while covered; avoids exposing the whole desktop),
  or DXGI Desktop Duplication for a full screen. Both return GPU textures; Duplication also returns dirty and move
  rectangles. No new frame means no encode and no packet.
- Encode: Media Foundation H.264 with a D3D11 texture as input (hardware where present, Microsoft's software
  encoder as fallback; no third-party codec licence). Low-latency settings, no B-frames, constant bitrate.
- Zero copy and zero alloc: textures stay in video memory from capture to encode; only the compressed bitstream
  (a few KB per frame) is copied, once, into the encrypted packet. Use a fixed pool of 2 to 3 textures, reused
  sample buffers, a packet pool and a single-producer single-consumer ring between capture and send. Windows and
  Media Foundation may allocate internally; the target is no allocation in our code in steady state, enforced by a
  test with a counting allocator.
- Transport: a `str0m` video track on the WebRTC path (hardware decode on phones); the WebSocket fallback needs
  WebCodecs frames, which must be verified against the iOS 16.4 floor (DECISION-0112). If WebCodecs is not
  acceptable there, fall back to tiled WebP over the data channel.
- Input: `SendInput` from the user-session helper, owner only, and per window opt-in.
- Quality: hardware H.264 is 4:2:0, so coloured text blurs; acceptable for browser pages, and the terminal stays
  cell/text based and unaffected.

### Not in scope

Windows' own RDP host (not on Home, takes over the interactive session, a second protocol and port outside the
identity and path model of DECISION-0123). Protected (DRM) content stays black.

### Spike (decides the phase)

Capture one window, encode with the hardware encoder, send over a `str0m` video track to a phone browser. Measure:
end-to-end latency, bitrate during a normal coding session, idle cost (should be zero packets), CPU and memory on
the 512MB / 1 vCPU budget (GUARDRAIL-resource-constraints), and `str0m` congestion control behaviour.

### Done when

A phone shows and drives one window of the Windows host, idle sends nothing, and the spike numbers are recorded
as facts.

## 3. Secure desktop (only if P1 is B or C)

### Why a capture from the user session fails

The secure desktop is a separate Windows desktop that user-level processes cannot read. Duplication returns an
access-lost error when it becomes active, and Graphics Capture does not see it either, so a user-session stream
freezes or goes black.

### Phase 2 behaviour regardless of P1

Detect the switch (`ACCESS_LOST` plus the input desktop name) and show "a secure prompt is open on the PC; approve
it there". This is the whole behaviour if P1 is A.

### Components (B and C)

1. A SYSTEM service, installed once by an administrator, that watches session changes.
2. A helper per session, started by the service with a SYSTEM token duplicated from `winlogon.exe`, attached to the
   interactive desktop. It polls `OpenInputDesktop`, calls `SetThreadDesktop` on change, recreates the duplication
   object and keeps encoding. It covers the lock and sign-in screens too.
3. For C only: input on the secure desktop through the same helper; Ctrl+Alt+Del through `SendSAS` (`sas.dll`),
   which needs the software-SAS-generation policy to allow services; input into already-elevated normal windows
   needs a `uiAccess=true` manifest, a signed binary and an install under Program Files.

### Controls (required before shipping B or C)

- A separate, explicit opt-in at install time and per server; off by default.
- For C: input to the secure desktop is gated apart from ordinary display input (a confirmation or a time-limited
  grant).
- Audit events record that the secure desktop was viewed or driven, never its content or keystrokes
  (GUARDRAIL-audit-logging-no-secrets, GUARDRAIL-security-secrets-handling). The password a user types is never
  logged or stored.
- Agents cannot request, observe or drive it. Nothing in the agent-facing protocol exposes the helper.
- The service and helper are separate binaries from the server and talk to it over a local named pipe with an
  ACL that admits only the service account and the owner (GUARDRAIL-internal-network-binding).
- Uninstall removes the service, helper, pipe and policy changes.

### Risks

- Endpoint protection flags a SYSTEM service that captures other desktops and injects input. Expect a
  code-signing certificate (waived under GUARDRAIL-no-paid-services as a distribution cost) and vendor
  submissions. Defender's current behaviour is unchecked.
- A new installer (MSI or equivalent) with admin rights, a service upgrade path and cleanup.

## 4. Order and gates

| Step | Gate to start | Output |
|---|---|---|
| Answer P1 to P4 | none | A decision record |
| psmux spike (1 day) | decision | Go or no-go on reusing tmux code |
| Phase 1 | backend choice | Windows server release, terminal only |
| GUI spike | phase 1 in CI | Numbers recorded as facts |
| Phase 2 | spike within budget | `display` windows for one window |
| Phase 3 | P1 is B or C, and phase 2 shipped | Service, helper, controls |

## 5. Knowledge-base changes

- A decision for P1 to P4, and one for the backend choice.
- If an in-process backend replaces tmux on Windows only, no existing decision is superseded; if it later replaces
  tmux everywhere, DECISION-0119, 0120, 0121 and 0149 are superseded.
- Facts from the spikes (latency, bitrate, memory, CPU).
- A guardrail for phase 3 if P1 is B or C: the secure-desktop controls above, as a rule, not a note.
- Update the minimum-platform statement in the instruction file when Windows ships.
