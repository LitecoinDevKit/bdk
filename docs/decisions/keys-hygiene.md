# Keys: secrets are redacted, wiped, and scrubbed before extract

## Decision

Scan/spend keys and coin secrets (`blind`, `shared_secret`, `spend_key`) do not appear in `Debug`, are wiped on drop, and are stripped from PSBT maps before extract/broadcast. `Secret32` prints `Secret32(..)` and compares in constant time. `MwebCoin` keeps raw `[u8; 32]` fields so downstream struct literals still compile.

## Why

`MwebCoin` docs: those three fields are spend-equivalent; a stray `{:?}` in a caller log must not leak them; fields stay `[u8; 32]` for API compatibility. `Secret32` / `MasterKeys` docs: wipe is best-effort (moves, `Vec` realloc, swap defeat it). `scrub_sensitive_fields`: remove amount / shared secret / address index / `mweb()` descriptor (scan WIF) before extract.

## How we'd know it broke

- `debug_redacts_scan_and_spend` / `drop_erases_scan_and_spend` in `crates/mweb/src/keys.rs`.
- `debug_redacts_spend_equivalent_fields` / `drop_wipes_secrets_in_place` in `crates/mweb/src/coin_db.rs`.
- `debug_does_not_print_the_secret` in `crates/mweb/src/secret.rs`.
- `fund_sign_extract_emits_stealth_and_scrubs` in `crates/mweb/src/psbt.rs` — after `sign_funded_mweb`, `amount`, `shared_secret`, `address_index`, and `address_descriptor` are `None`; serialize round-trip stays scrubbed.
- `sign_funded_mweb` / `sign_funded_mweb_pegin` in `crates/mweb/src/psbt_fund.rs` both call `scrub_sensitive_fields` before return.
