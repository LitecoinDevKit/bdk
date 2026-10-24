# Broadcast: MWEB-only goes to a node that keeps `mw_tx`; identify by kernel / wtxid

## Decision

Pure MWEB txs (empty canonical vin/vout) are not identified by the empty-skeleton `txid`. `TcpMwebPeer::broadcast_tx` sends the consensus-encoded `tx` (MWEB body in segwit flag `0x08`) and polls `getdata` with `MSG_MWEB_TX` + the first kernel id (`tx_inventory`). Hybrid / peg-in txs use ordinary txid. Node tests broadcast with `sendrawtransaction`. Electrum `transaction_broadcast` is the transparent path and is not the MWEB-only path.

## Why

`broadcast_tx` / `tx_inventory` comments in `crates/mweb/src/lip0006_tcp.rs`: Electrum cannot relay pure MWEB; empty vin/vout share one `txid` (Core `ComputeHash` special-cases kernel hash). `docs/MWEB_PEER_OPS.md` / `docs/MIGRATE_FROM_MWEBD.md`: Esplora often accepts the empty shell; prefer local `sendrawtransaction` and track by **wtxid**.

## How we'd know it broke

- `tx_inventory` in `crates/mweb/src/lip0006_tcp.rs` — empty vin/vout must be `Inventory::Unknown { MSG_MWEB_TX, kernel_id }`, not `compute_txid()`. There is no unit test on this branch today; a flip to txid would be silent until a live poll.
- `probe_mweb_tx_getdata_notfound` (ignored) in the same file — node must accept `MSG_MWEB_TX` inv encoding (`notfound` for a random hash).
- `crates/mweb/tests/core_spend.rs` — `send_raw_transaction` of a BDK-authored spend, then Core `list_received_by_mweb_address` credits the payee.
- `crates/mweb/tests/core_pegin_pegout_roundtrip.rs` — BDK peg-in/peg-out mempool-accepted; HogEx carries the peg-out SPK.
- Library does not enforce “RPC first vs Esplora”; that policy lives in the sibling wallet / docs. Electrum `transaction_broadcast` in `crates/electrum/src/bdk_electrum_client.rs` returns `Txid` and will mis-identify a pure MWEB send if used as the only path.
