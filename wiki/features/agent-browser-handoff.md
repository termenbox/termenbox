# When an agent is stuck in a browser

How an agent that drives a browser on your server's machine asks you for the step only a person can do: a login, a second factor, a CAPTCHA, a consent wall or a payment confirmation. Back to the [README](../../README.md). Related: [Notifications and agent events](./notifications.md), [Configuration](../reference/configuration.md).

Termenbox does not start, show or control the browser. The agent runs its own browser window on that machine and you do the step there. What Termenbox adds is the ask: the session shows as waiting for you, the card names the site and the reason, and the agent goes on when you tap **Done**, or stops waiting when you tap **Can't**. A phone tells you what is blocked and where, and can refuse; it cannot solve the step.

## Turn it on

It is off. Set `TERMENBOX_HANDOFF=on` and restart the server. With it off, every request from an agent is refused and nothing shows.

## Keep the agent's browser in a profile used for agents only

Give the agent's browser a profile of its own, one you use for nothing else, and sign in there only to the sites the agent needs. Do not point the agent at the browser you use every day: the agent can read and click in every tab and signed-in session of the profile it drives, and a page it reads can try to steer it. A profile used for agents only limits what a misled agent can reach.

## What the agent runs

From the agent's shell, in a Termenbox tab:

```
termenbox-server handoff <login|two_factor|captcha|consent|payment|other> <site> [note]
```

The site is a host name (`example.com`); anything after it (a path, a query, a port, a password) is dropped. The note is at most 140 characters, with addresses and secret-looking text removed. The command waits about 100 seconds and prints one line: `resolved` (exit 0), `declined` (exit 10), `timeout` (exit 11) or `pending` (exit 12). On `pending` the agent runs it again; that joins the same handoff and does not extend its 30 minutes.

Only the agent of that pane can ask. A command run from any other process is refused, and a pane with no local agent process (over ssh or in a container) cannot use it. Only your own signed-in client can answer; the command has no way to.

## What you see

The card shows the site in full (an international name in its ASCII form, so a look-alike is plain to see), the reason, the note as plain text, the name of the machine and the time left. A push says only that an agent is waiting, with no site, reason or note. The audit log keeps a hash of the site, never the site or the note. A restart of the server closes every open handoff.
