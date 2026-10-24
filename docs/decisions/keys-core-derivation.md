# Keys: Core HD paths by default

## Decision

`MasterKeyScheme::LitecoinCore` (`m/0'/100'/{0,1}'`, BLAKE3 `'A'` tweak) is the default. `master_keys_from_seed` hard-wires it. LIP-0004 and mwebd/Nexus paths exist only as explicit `MasterKeyScheme` variants.

## Why

Matches Litecoin Core 0.21 `LoadMWEBKeychain` / `mw::Keychain::GetSpendKey` (`crates/mweb/src/keys.rs` module docs). Reviewer's Guide invariant 6: Core-compatible stealth by default, not LIP-0004 alternate paths.

## How we'd know it broke

- `core_mweb_addresses_match_bdk_after_sethdseed` in `crates/mweb/tests/core_seed_parity.rs` — Core `getnewaddress "" "mweb"` (indices 2–5) ≠ BDK `keys.address`.
- `MasterKeyScheme` `#[default]` + `master_keys_from_seed` in `crates/mweb/src/keys.rs` — a default flip or a convenience helper that picks `Lip0004` / `Mwebd` silently changes every address.
- `address_index_tweak` in the same file — wrong tag or endianness desyncs Core `GetSpendKey` even if the BIP32 path stays.
