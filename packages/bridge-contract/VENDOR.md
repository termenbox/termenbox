# Vendored: bridge-contract 0.6.0

Source: `paper/supplementary/rust-bridge/` of the Bridge Contract paper repository (commit `8c7224c`: proof of possession is an Ed25519 signature, the key registry holds public keys only, `Ed25519Verifier` replaces the HMAC stand-in; vectors V01-V43 unchanged),
with the language-neutral vectors from `paper/supplementary/vectors/` copied to `vectors/`.

Local changes (keep this list complete; re-vendor by copying upstream and re-applying it):

- `Cargo.toml`: no empty `[workspace]` table; `serde_json`, `sha2` come from the workspace (`ed25519-dalek`, `getrandom`, `hex` as upstream).
- `src/`, `tests/`: reformatted with `cargo fmt` to satisfy the workspace's format check (no behaviour change; re-run `cargo fmt -p bridge-contract` after re-vendoring).
- `tests/vectors.rs`: vectors are read from `vectors/` (next to `src/`) instead of `../vectors` (the comment and the `join` only).

Do not edit `src/` here (apart from formatting). A behaviour change goes upstream first, with a vector. The README's independence caveats
apply: this crate and the Python reference share an author, so passing the vectors shows consistency, not
independent confirmation.
