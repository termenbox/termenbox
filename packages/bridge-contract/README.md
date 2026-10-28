# bridge-contract (Rust)

A second implementation of the Bridge Contract, revision -05: the product
state machine `Attachment x Human x Lifecycle` derived from a client set
(I15), challenge-response resume pinned to the acknowledged prefix of the
chain (I4, I9, I11), narrow-only authority with enforced and observed
changes (I3, I10), durable append (I12), restart recovery by log replay,
the tool queue (I13) and answer binding (I14).

It is a standalone crate with no dependency on Termenbox (decision D8); a
Termenbox integration vendors it as a dependency.

```
cargo test        # runs ../vectors/*.json on two storages, plus unit tests
```

## API for a consumer

| Piece | What it is |
|---|---|
| `Storage` (trait, `Send + Sync`, `&self`) | `append_batch` (atomic, durable on return), `read_log`, `sessions`, and the client-key registry (`register_key`, `key_material`). `MemStorage` is the reference implementation; the tests also run a file-backed one (`tests/common/mod.rs`: JSON lines, `fsync` per batch). |
| `Verifier` (trait, `Send + Sync`) | Proof-of-possession check over `(key material, message, signature)`. `HmacVerifier` is the stand-in used by the tests and vectors; a consumer plugs in a real signature scheme and registers public keys as the key material. The signed message is `request_message(sid, nonce, seq, hash)`. |
| `Clock` (trait, `Send + Sync`) | `SystemClock` for production, `FakeClock` for tests. |
| `Config` | `T_call <= T_human` and `T_detach <= T_total` are checked by `Config::new`, `Bridge::new` and the profiles `Config::interactive_short()` and `Config::persistent()`. |
| `Bridge` | `Send`. Methods take `&mut self`; share one across threads as `Arc<Mutex<Bridge>>` (the tests do), or one bridge per session. |

The bridge keeps nothing a restart cannot rebuild from the `Storage`, except
the challenge nonces, which are volatile by design (I11b).

## Configuration beyond the timers

`Config::with_detached_calls(policy, allow)` selects what a call that arrives
with no escalation pending while no client is attached gets (G1): `Refuse`
(default), `Queue` (held until answered after re-attach) or `Forward` (an
allowlisted, in-scope call is forwarded at once). `Bridge::delivered` and
`Bridge::completed` record the runtime's acknowledgement; a drained call that
was not delivered is forwarded again by a bridge instance that has not yet
forwarded it, when the session is back in `(Att, None)`.

## What it checks

`tests/vectors.rs` consumes the same `../vectors/V01.json` ... `V40.json` that
`../run_vectors.py` runs against the Python bridge, once through `MemStorage`
and once through the file-backed storage. The vectors compare committed event
kinds, the final cell, the scope version, the queue, the forwarded calls, the
client set, the quarantine flag and the number of revert requests. Every vector passes on both storages.
Further tests check that `Bridge`, the storages, the clocks and the verifier
are `Send` (and `Sync`), and that a bridge shared behind a mutex serves
sessions from several threads. As a sanity check, deliberately breaking the
prefix check or the answer-hash comparison makes the vectors fail (V02, V25).

## Independence caveats

* Both implementations have the same author (and were produced in the same
  working session, after the Python bridge). The crate was written from the
  paper's Sections 4 and 5 and the vector schema, not by translating
  `conformance_suite.py`, but the author's reading of the specification is
  shared. Agreement between the two shows consistency of one reading, not
  independent confirmation of the specification. This holds for the changes of
  revision -05 that came from the Termenbox integration as much as for the
  rest: the same author wrote the product plan and the specification change.
* The architecture is the same: the cell is a fold of the committed log and a
  transition is an atomic append followed by an in-memory apply. That is a
  design choice of the specification's I7/I12, not an accident.
* Several choices are not forced by the paper text and were fixed by the
  vectors: the order of the request checks (transport replay, registered key,
  signature, nonce, prefix), the `RESUME_REJECTED` reasons, and the event
  order inside a batch. A third implementation could differ on these and
  still be conformant; if so, the vectors, not this crate, are the arbiter.
* Proof of possession uses an HMAC stand-in for a signature (see
  `../vectors/README.md`); no asymmetric cryptography is exercised. The
  `Verifier` trait is the seam for it, and it has been exercised only with the
  stand-in.
* The file-backed storage is a test fixture: no cross-process locking, no
  compaction, and `read_log` reads the whole file. A production `Storage`
  needs its own answer to those.
* The hash-chain encodings are not byte-compatible with the Python bridge
  (float formatting differs) and need not be: the vectors compare event
  kinds, not hashes.
* Not implemented: I-CHECKPOINT (SHOULD), I-SIGN-EVENT (SHOULD), I8's source
  of truth (the transport reports a replay through a flag), and the bridge's
  rate limiting of challenges and rejections (a SHOULD of I11).
