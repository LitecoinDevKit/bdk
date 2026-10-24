# Sync source: default is chain-anchored; env cannot select Trusted

## Decision

`VerifyMode::default()` is `Anchored`: the LIP-0006 peer’s `mwebheader` is bound to a caller-supplied `block_hash` through HogEx before leafset/UTXO roots are trusted. `BDK_MWEB_VERIFY_MODE` may downgrade only to `HeaderAndPmmr`. `Trusted` is not reachable from the environment. `block_hash` must come from a header chain the caller trusts independently of the MWEB peer.

## Why

`VerifyMode` and `parse_verify_mode` docs in `crates/mweb/src/lip0006.rs`: `HeaderAndPmmr` is self-consistency only (same peer chose the roots); an env var that selected `Trusted` would be a downgrade attack. `docs/SECURITY_PLAN.md` F-01: without the HogEx bind, a peer can present an invented UTXO set (omit coins or inject phantom balance).

## How we'd know it broke

- `default_is_anchored_unless_the_escape_hatch_is_set` / `the_escape_hatch_cannot_select_trusted` / `the_syncer_inherits_the_default_verify_mode` in `crates/mweb/src/lip0006.rs`.
- `only_anchored_binds_the_header_to_the_chain` — `Trusted` / `HeaderAndPmmr` are not `is_anchored()`.
- `crates/mweb/tests/mweb_anchoring.rs` — `Anchored` accepts a real litecoind `mwebheader` and rejects wrong block hash, tampered `output_root` / `leafset_root`, and HogEx swapped for another tx.
- `verify_mode_rejects_tampered_leafset` in `crates/mweb/tests/lip0006_sync.rs`.
- `walk_down_cannot_smuggle_tampered_data_at_width_one` in `crates/mweb/src/mweb_sync.rs`.
