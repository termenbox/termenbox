---
id: 0168-the-agent-view-tells-what-happened-while-away-flags-a-stuck-agent-and-queues-the-next-prompt
title: The Agent view tells what happened while the user was away, flags a stuck agent, and queues the next prompt
status: active
date: 2026-11-01
track: product
accepted-by: Sergey Strebulaev <s@vivantel.dev>
tags: [client, acp, ux, automation, mobile]
related: [0122, 0137, 0163, 0164]
---

## Decision

- **A digest on return.** When the user reopens a tab after being away, a card above the stream says, from the stream since the last place: turns finished, files changed, questions answered by rules (0180), errors, and the final answer of the last turn. It is computed on the device from the stream, dismisses with one tap, and appears only when something happened. It uses the same turn summaries as Outline.
- **A stuck signal.** A turn that shows no output or tool progress for a threshold (default 5 minutes, per device) while the state is "running" shows a notice in the strip with the time since last activity and Stop; it does not interrupt anything. It is a display, not a server action.
- **A queued next prompt.** While a turn runs the composer accepts a prompt, shows it as a chip ("next: …"), and the server sends it when the turn ends. The queue is held by the server so it survives a locked phone and a reconnect; it holds at most five prompts, each can be edited or removed until it is sent, and a server restart drops the queue with a visible notice (as it loses a running turn, 0137). A failed or cancelled turn pauses the queue and asks the user, so a queued prompt never runs on top of a failure.

## Why

The phone user is away from the screen most of the time. A digest answers "what happened", the stuck signal answers "is it hung", and the queue lets the next instruction be given while the agent works, which is the biggest convenience gain after the permission rules.

## Not chosen

- A device-only queue (lost when the phone locks), an unbounded queue, auto-running a queue after a failure, and a server-side digest or push text now (needs the turn reducer's wording; left as a follow-up).

## Consequences

- A new wire message for queueing and an optional queue field on the stream state; both optional so old clients keep working.
