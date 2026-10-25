# bridge-contract (Rust)

A second implementation of the Bridge Contract, revision -05: the product
state machine `Attachment x Human x Lifecycle`, the authenticated single-use
resume token (I11), narrow-only authority (I3, I10), token TTL (I9), durable
append (I12), restart recovery by log replay, and the tool queue (I13).

It is a standalone crate with no dependency on Termenbox (decision D8); a
Termenbox integration vendors it later.

```
cargo test        # runs ../vectors/*.json plus three unit tests
```

## What it checks

`tests/vectors.rs` consumes the same `../vectors/V01.json` ... `V20.json` that
`../run_vectors.py` runs against the Python bridge. The vectors compare
committed event kinds, the final cell, the scope version, the queue and the
forwarded calls. All 20 pass. As a sanity check, deliberately breaking the
approval-expiry test, the MAC check or the "last issued token" rule makes the
vectors fail (V14, V02, V07).

## Independence caveats

* Both implementations have the same author (and were produced in the same
  working session, after the Python bridge). The crate was written from the
  paper's Sections 4 and 5 and the vector schema, not by translating
  `conformance_suite.py`, but the author's reading of the specification is
  shared. Agreement between the two shows consistency of one reading, not
  independent confirmation of the specification.
* The architecture is the same: the cell is a fold of the committed log and a
  transition is an atomic append followed by an in-memory apply. That is a
  design choice of the specification's I7/I12, not an accident.
* Several choices are not forced by the paper text and were fixed by the
  vectors: the order of the re-attach checks (replay, MAC, session, last-issued
  token, scope version, proof, expiry), the `RESUME_REJECTED` reasons,
  and the event order inside a batch. A third implementation could differ
  on these and still be conformant; if so, the vectors, not this crate, are
  the arbiter.
* Proof of possession uses an HMAC stand-in for a signature (see
  `../vectors/README.md`); no asymmetric cryptography is exercised.
* The hash-chain and MAC encodings are not byte-compatible with the Python
  bridge (float formatting differs) and need not be: the vectors compare
  event kinds, not hashes.
* Not implemented: I-CHECKPOINT (SHOULD), I-SIGN-EVENT (SHOULD), I8's source
  of truth (the transport reports a replay through a flag).
