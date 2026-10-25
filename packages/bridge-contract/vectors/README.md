# Language-neutral test vectors

One scenario per `Vnn.json`. A runner builds a bridge, creates the session
from `session`, applies `actions` in order, then checks `expect`.

## Schema

```
{ "id", "title", "covers": [...],
  "config":  { t_detach, t_human, t_call, t_approve, t_nonce, q_max }   // seconds / count; optional, defaults below
  "session": { session_id, principal_id, authority_scope, client_key_id },
  "actions": [ {"do": ..., ...} ],
  "expect":  { "events": [kinds], "cell": [...], "scope_version": n,
               "queue": [keys], "forwarded": [keys], "clients": [key ids],
               "reverts": n, "last_event_payload": {k: v} }   // or {"config_error": true}
}
```

Defaults: `t_detach` 1800, `t_human` = `t_detach`, `t_call` = `t_detach`,
`t_approve` 900, `t_nonce` 60, `q_max` 64. The clock starts at 1000 and moves
only on `advance`. `config_error: true` means constructing the bridge with
`config` must fail (`T_call <= T_human <= T_detach` is a MUST).

`cell` is `["Att"|"Det"|"Res", "None"|"Pend"]`, or `["Terminated"]`.
`events` is the whole committed log of the session, kinds only, in order.
`clients` is the connected client set, sorted.

## Actions

Clients are named by key id; `ck-1` is the key registered at attach. A client
"observes" every event committed while it is connected, and again when `sync`
or `disconnect` names it; its next request acknowledges that last event.

| `do` | fields | calls |
|---|---|---|
| `disconnect` | `client?` | transport lost for one client, or for every connected client |
| `sync` | `client` | the client has observed every event so far |
| `escalate` | | agent escalates to a human |
| `resolve` | `answer?` (`all` default, or `none`) | answer every unanswered queued call with the hash the bridge recorded (a faithful client), then human resolves |
| `answer` | `key`, `hash?` (`shown` default, or `other`), `approve?` | human decision on a queued call |
| `tool_call` | `key`, `tool`, `permission?`, `content?` | tool call submitted; `content` is what the human is shown |
| `narrow` | `scope` | propose a scope (narrowing; a widening is rejected) |
| `extend` | `scope`, `source?` (`requested` default, or `observed`) | propose a scope (a widening is rejected, or with `observed` reported and a revert requested; a subset is a no-op) |
| `add_client` | `client` | an attached client registers a further key |
| `challenge` | `as` | request a nonce and keep it under a name |
| `reattach` | `client?`, `nonce?`, `as?`, `prefix?`, `proof?`, `transport_replay?`, `complete?` | begin a re-attach; unless `complete: false`, complete it when the cell is Res |
| `join` | same as `reattach` | a further client joins an attached session |
| `complete_resume` | | finish verification (Res to Att) |
| `advance` | `seconds` | advance the clock, then evaluate timers |
| `tick` | | evaluate timers |
| `restart` | `downtime?` | crash the process, let `downtime` seconds pass with no timer evaluated, recover from the durable log |
| `tamper_log` | `index`, `field: "prev_hash"` | corrupt the stored event at `index` |
| `terminate` | | end the session |

Common optional fields on any action: `error: true` (the action MUST be
refused; default is that it MUST succeed) and `then_cell` (assert the cell
after the action).

`reattach` / `join` fields: `nonce` is `fresh` (default: request a challenge
first), `unknown` (a nonce the bridge never issued), or the name of a nonce
kept by `challenge` or by an earlier request's `as`. `prefix` is `seen`
(default: the last event the client observed), `tip`, `hash-mismatch` (right
sequence, wrong hash), or `beyond` (a sequence the chain has not reached).
`proof` is `valid` (default) or `invalid`. A `client` that was never
registered signs with a key of its own.

## Test-harness convention for proof of possession

Not part of the contract (the paper only requires a signature under a
registered client key). The vectors use a symmetric stand-in so a runner needs
only HMAC-SHA256:

* client secret: any 32 bytes registered at attach (or by `add_client`) under
  the client's key id;
* the signed message is the canonical JSON (sorted keys, no spaces) of the
  array `[session_id, nonce, seq, hash]`;
* valid signature: `hex(HMAC-SHA256(client_secret, message))`.

An `invalid` proof is any other string.
