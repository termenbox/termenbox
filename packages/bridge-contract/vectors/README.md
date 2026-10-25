# Language-neutral test vectors

One scenario per `Vnn.json`. A runner builds a bridge, creates the session
from `session`, applies `actions` in order, then checks `expect`.

## Schema

```
{ "id", "title", "covers": [...],
  "config":  { t_detach, t_human, t_token, t_call, t_approve, q_max }   // seconds / count; optional, defaults below
  "session": { session_id, principal_id, authority_scope, client_key_id },
  "actions": [ {"do": ..., ...} ],
  "expect":  { "events": [kinds], "cell": [...], "scope_version": n,
               "queue": [keys], "forwarded": [keys],
               "last_event_payload": {k: v} }   // or {"config_error": true}
}
```

Defaults: `t_detach` 1800, `t_human` 86400, `t_token` 900, `t_call` 3600,
`t_approve` 900, `q_max` 64. The clock starts at 1000 and moves only on `advance`.
`config_error: true` means constructing the bridge with `config` must fail
(the `T_token <= T_detach` rule).

`cell` is `["Att"|"Det"|"Res", "None"|"Pend"]`, or `["Terminated"]`.
`events` is the whole committed log of the session, kinds only, in order.

## Actions

| `do` | fields | calls |
|---|---|---|
| `disconnect` | | client transport lost |
| `escalate` | | agent escalates to a human |
| `resolve` | | human resolves |
| `tool_call` | `key`, `tool`, `permission?` | tool call submitted |
| `narrow` | `scope` | propose a scope (narrowing; a widening is rejected) |
| `extend` | `scope` | propose a scope (a widening is rejected, a subset is a no-op) |
| `reattach` | `token`, `proof`, `transport_replay?`, `complete?` | begin re-attach; unless `complete: false`, complete it when the cell is Res |
| `complete_resume` | | finish verification (Res to Att) |
| `advance` | `seconds` | advance the clock, then evaluate timers |
| `tick` | | evaluate timers |
| `restart` | `downtime?` | crash the process, let `downtime` seconds pass with no timer evaluated, recover from the durable log |
| `tamper_log` | `index`, `field: "prev_hash"` | corrupt the stored event at `index` |
| `terminate` | | end the session |

Common optional fields on any action: `error: true` (the action MUST be
refused; default is that it MUST succeed) and `then_cell` (assert the cell
after the action).

`reattach.token`: `current` (the last issued token), `previous` (the token
issued before it), `tampered` (the current token with its scope field
altered, so the MAC no longer matches). `reattach.proof`: `valid` or
`invalid`.

## Test-harness convention for proof of possession

Not part of the contract (the paper only requires a signature over a bridge
nonce and the token sequence). The vectors use a symmetric stand-in so a
runner needs only HMAC-SHA256:

* client secret: any 32 bytes registered at attach under `client_key_id`;
* nonce: what the bridge returns for the current token;
* valid proof: `hex(HMAC-SHA256(client_secret, nonce + "|" + sequence))`.

An `invalid` proof is any other string.
