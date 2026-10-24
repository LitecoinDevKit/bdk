# Peg-in/out: HogAddr v8 and peg-in v9 are not transparent UTXOs

## Decision

Witness v8 (HogAddr) and v9 (peg-in) are never indexed as spendable outs, even if their script bytes are on the watch list. Peg-out destinations in the same HogEx (ordinary p2wpkh/p2tr) still index when watched. MWEB coins live in `MwebCoinDatabase`, not `IndexedTxGraph`.

## Why

`crates/chain/src/litecoin_scripts.rs`: v8 is the HogEx integration balance (not user-spendable); v9 is funds entering the extension block. Reviewer's Guide / `docs/MWEB_ARCHITECTURE.md`: indexing them inflates transparent balance. Peg-outs must still index so a watched destination is credited.

## How we'd know it broke

- `detects_v8_and_v9_bridge_programs` / `ignores_ordinary_segwit` in `crates/chain/src/litecoin_scripts.rs`.
- `SpkTxOutIndex::scan_txout` in `crates/chain/src/indexer/spk_txout.rs` returns `None` for `is_mweb_bridge_output`.
- `spk_index_skips_bridge_outs_but_indexes_pegouts` in `crates/chain/tests/test_litecoin.rs` — watched HogAddr must not match; peg-out vout must; HogEx stays relevant via the peg-out alone.
- `spk_index_skips_synthetic_v9_pegin` in the same file.
- `hogex_first_output_is_mweb_bridge_hogaddr` — fixture HogEx vout0 stays classified as bridge.
