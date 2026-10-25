# Vendored: bridge-contract 0.5.0

Source: `paper/supplementary/rust-bridge/` of the Bridge Contract paper repository (commit `7aa7e39`, draft-05 as released in -05 (Storage/Verifier traits, client set, challenge-response resume)),
with the language-neutral vectors from `paper/supplementary/vectors/` copied to `vectors/`.

Local changes (keep this list complete; re-vendor by copying upstream and re-applying it):

- `Cargo.toml`: no empty `[workspace]` table; `hmac`, `serde_json`, `sha2` come from the workspace (`getrandom`, `hex` as upstream).
- `tests/vectors.rs`: vectors are read from `vectors/` (next to `src/`) instead of `../vectors` (the comment and the `join` only).

Do not edit `src/` here. A behaviour change goes upstream first, with a vector. The README's independence caveats
apply: this crate and the Python reference share an author, so passing the vectors shows consistency, not
independent confirmation.
