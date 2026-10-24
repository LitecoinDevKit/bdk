# Logging: operational counts and hashes only; secrets never print

## Decision

`eprintln!` in this crate prints verify-mode escape-hatch warnings, sync progress (leaf cursor, batch counts, tip hashes), reconnect/throttle warnings, and (in ignored tests) discovered peer socket lists. It does not print scan/spend keys, blinds, shared secrets, or spend keys. Secret-bearing types redact in `Debug` so a caller `{:?}` cannot leak them. `VERIFY_MODE_ENV=header-and-pmmr` logs that roots will not be anchored.

## Why

`MasterKeys` / `MwebCoin` / `Secret32` docs: `Debug` is structural so a stray log cannot leak key material. `parse_verify_mode`: the escape hatch must be loud. Sync `eprintln`s in `mweb_sync.rs` are progress (tip hash, leaf index, batch size) — public chain data.

## How we'd know it broke

- `debug_redacts_scan_and_spend` (`keys.rs`), `debug_redacts_spend_equivalent_fields` (`coin_db.rs`), `debug_does_not_print_the_secret` (`secret.rs`) — hex of the secret must not appear in `format!("{:?}", …)`.
- `parse_verify_mode` in `crates/mweb/src/lip0006.rs` — unrecognised / `header-and-pmmr` values `eprintln!` the env name and the posture; they must not dump keys.
- Grep `eprintln!` in `crates/mweb/src/{lip0006.rs,mweb_sync.rs,lip0006_tcp.rs,discovery.rs}` — current format strings are counts, hashes, addrs, error display. Adding `{keys:?}` or `{coin:?}` without the redacting impls is the break; the Debug tests above are the gate.
- `discovers_live_mainnet_peers` (ignored) prints peer addresses, not secrets.
