# MWEB audit checklist: LDK vs Litecoin Core 0.21.5.8

Scope: the MWEB funds path in `bdk_mweb`, the pinned `rust-litecoin` codecs and
PSBT extract (`5a0233712bdefc568fb45d71f42c028e196c4352`), and the `bdk_wallet`
facade, checked against Litecoin Core 0.21.5.5 through 0.21.5.8.

One finding (P-1) is under embargo. This file names it and tracks its status
only; reproduction details are in the private report and stay out of this
public repository until a fix ships. See "Embargo" at the end.

Status key: `[x]` done, `[ ]` open, `[~]` partially done.

## Reference and evidence environment

- [x] Core `v0.21.5.7` (`b1150f51`) and `v0.21.5.8` (`ec1b6489`) fetched; clean
      reference worktree at `v0.21.5.8`. Both tags carry a good signature from
      David Burkett's key `D35621D5 3A1CC6A3 456758D0 3620E9D3 87E55666`
      (not web-of-trust certified on the audit machine).
- [x] 0.21.5.8 release binaries checked against the signed `SHA256SUMS.asc`
      (same key). Linux x86_64: `43200c9f…1ab1be9`; macOS x86_64: `32fda6b9…7be3b`.
- [x] `regtest_mweb.yml` pinned to 0.21.5.8.
- [x] Strict relay-policy node mode: `LITECOIND_RELAY_POLICY=strict|permissive`
      in `crates/testenv/src/litecoin_regtest.rs`; CI runs every node suite in
      both. Before this, the harness always passed `-acceptnonstdtxn=1`, which
      also switches off Core's MWEB relay policy (`MWEB::Policy::CheckWeight`,
      `IsStandardTx`), so "Core accepts what we author" was proven for
      consensus only.
- [x] All node-backed suites pass on 0.21.5.8 in both modes (13 tests:
      bulletproof gate, receive scan, seed parity, spend, peg-in/peg-out
      roundtrip, LIP-0006 sync/p2p/throttle, anchoring). Spends, peg-ins and
      peg-outs go through `testmempoolaccept`/`sendrawtransaction`, so strict
      mode proves they are relay-standard.
- [x] Fuzz crate re-pinned from `0b32853` to the shipped `5a02337` (it was one
      commit behind, and that commit touches `mimblewimble.rs`).
- [x] Both soft-fork heights have passed on mainnet (tip 3,205,231 on
      2026-10-31; empty-pegout rule at 3,154,440, empty-extra-data rule at
      3,172,640).
- [ ] Mainnet archive node used for the audit runs 0.21.5.5, which predates
      both soft forks. Upgrade it (and any LIP-0006 peer, header source and
      broadcast node) to 0.21.5.8.

Note on Core commit IDs: 0.21.5.6 shipped as a single squashed commit
(`992c5272e`). The per-fix commits cited below for that release
(`cfdfcf57aa`, `b250b016a9`, `67580d6e35`, `109ed13b73`, `dc72334202`,
`4dd6aeeb97`, `136695e050`) exist upstream but are not ancestors of the tag.

## How the 2026 Core fixes map onto LDK

| Release | Core change | LDK exposure | Finding |
| --- | --- | --- | --- |
| 0.21.5.5 | `773c138f78` `SecretKey::FromHash` fallback | not ported | P-8 |
| 0.21.5.5 | `1cd94bbcb6` amount hardening, MWEB fee = weight x 100 | no fee floor | P-2 |
| 0.21.5.5 | `8f8ad64d04` duplicate peg-in guards | LDK authors one kernel per peg-in | checked |
| 0.21.5.5 | `a549004385` kernel lock height enforced | LDK never sets one; extract can carry one | P-5 |
| 0.21.5.5 | `c25bf8923c` peg-out scripts must be standard | no peg-out script checks | P-2 |
| 0.21.5.5 | frozen MWEB output IDs | not excluded | P-4 |
| 0.21.5.6 | `cfdfcf57aa` | embargoed | P-1 |
| 0.21.5.6 | `b250b016a9` / `67580d6e35` empty-pegout soft fork | extract can emit it; kernel ID differs | P-5, P-9 |
| 0.21.5.6 | `109ed13b73` / `dc72334202` 4,000 weight / 1,000 input relay cap | no cap | P-2 |
| 0.21.5.6 | `4dd6aeeb97` / `136695e050` mutated blocks | node-side; covered by anchoring | checked |
| 0.21.5.7 | `a399fc4ca7` / `b4313c34d2` empty-extra-data soft fork, revalidate connected body | extract can emit it; kernel ID differs | P-5, P-9 |
| 0.21.5.7 | `d3b9ac3969` MWEB-only mempool txs hidden at `rpcserialversion<=1` | emitter check open | open |
| 0.21.5.8 | `d7ab03d31c` bulletproof scratch-frame leak | present, cannot accumulate | P-10 |
| 0.21.5.8 | `c07d6228c0` recentRejects censorship of MWEB txs | broadcast via 0.21.5.8+ | operational |
| 0.21.5.8 | `f6afd26826` / `70ea696f36` / `0cb1419649` / `36f06455f9` | node-side | feeds P-6 |
| 0.21.5.8 | `63ba41f50e` DNS seed replaced | stale seed | P-13 |

## Findings

Severity reflects the evidence gathered in this pass. "Red test" names the
ignored test that asserts the fixed behaviour; run it with `--ignored`.

| ID | Severity | Status | Evidence |
| --- | --- | --- | --- |
| P-1 | High (embargoed) | confirmed on regtest; not present on mainnet (UTXO set and full block history checked); fixed locally, unreleased | private report |
| P-2 | Medium | confirmed | `tests/policy_guardrails.rs` (red), `tests/core_policy_guardrails.rs` (Core behaviour) |
| P-3 | Low | confirmed | `crates/chain/tests/test_canonical_view.rs::test_hogex_pegout_is_immature_for_six_blocks` (red) |
| P-4 | Low | confirmed; the frozen output is in the mainnet UTXO set (leaf 308,526) | `tests/coin_db_parity.rs` (red) |
| P-5 | Low | confirmed | `tests/kernel_codec_parity.rs` (6 red), fuzz `psbt_extract --features audit-red` |
| P-6 | Medium | confirmed | `mweb_sync::tests::differential_sync_restores_coin_whose_spend_never_confirmed` (red) |
| P-7 | Low (was "possibly High") | analysed | see below |
| P-8 | Low | confirmed (parity) | code reading |
| P-9 | Low | confirmed; no mainnet instances | `tests/kernel_codec_parity.rs::noncanonical_kernel_id_matches_core` (red) |
| P-10 | Info | confirmed | code reading |
| P-11 | Low | confirmed | `tests/coin_db_parity.rs` (red) |
| P-12 | Low | confirmed | `tests/coin_db_parity.rs` (red) |
| P-13 | Low | confirmed | `discovery.rs` lists `dnsseed.litecoinpool.org`; Core uses `dnsseed.ltcpool.org` |

Independent review of the Tier 1 files added:

| ID | Severity | Status | Location |
| --- | --- | --- | --- |
| R-1 | High (embargoed) | reproduced on regtest; not present on mainnet; fixed locally, unreleased | private report |
| R-2 | Medium (embargoed) | confirmed by code reading; fixed locally, unreleased | private report |
| R-3 | Medium (embargoed) | confirmed by code reading; fixed locally, unreleased | private report |
| R-4 | Medium | reported | rust-litecoin `Transaction::compute_wtxid` hashes MWEB data; Core's `GetWitnessHash` uses `SERIALIZE_NO_MWEB`, so `check_witness_commitment` and BIP152 short IDs disagree with Core |
| R-5 | Low | reported | `compute_txid` for MWEB-only txs is not Core's first-kernel hash; all such txs share one txid |
| R-6 | Low | reported | PSBT `combine` merges MWEB maps by position; `sign_funded_mweb` re-sorts inputs |
| R-7 | Low | reported | `sign_mweb_components` signs with an ephemeral key the existing stealth offset does not include, so anything it signs is rejected |
| R-8 | Low | reported | `Debug` on `StagedMwebOutput` / `FundedMwebPsbt` prints blinds and sender keys; `sign_funded_mweb` holds keys outside `Zeroizing` |
| R-9 | Info | reported | tx flag `0x09` with no inputs, and MWEB presence bytes 2-255, parse where Core rejects or reads differently |

Notes:

- **P-2.** Core 0.21.5.8 accepts a fee of exactly weight x 100 and rejects one
  litoshi less, in both relay modes. It relays an OP_RETURN peg-out even under
  strict policy (`NULL_DATA` is standard and exempt from dust), so a caller
  who passes one burns the value. Non-standard peg-out scripts and txs over
  4,000 weight are refused only in strict mode.
- **P-5.** A PSBT kernel fee above `i64::MAX` reaches the wire as 1 litoshi.
  Kernel signatures commit to the fee, so the tx is rejected rather than
  mispaying; the risk is a misleading displayed fee and rejected sends. LDK's
  own fund-then-sign path populates the maps from canonical wire structs.
- **P-7.** `sign_mweb_components` signs each input with a fresh ephemeral key it
  never reveals, so a coordinator cannot compute the per-input `ki - ko` term
  the stealth sum needs; the signatures cannot be reused in a transaction the
  signer did not balance. What remains: the returned PSBT keeps each input's
  `amount` and `shared_secret` (which yields the input's blinding factor), and
  the facade wrapper will also sign inputs for coins already marked spent.
  R-7 makes the same point from the other side: because the fresh ephemeral
  key is not folded into the stealth offset, anything this function signs is
  rejected by Core.
- **P-10.** grin 0.7.15 misses two `secp256k1_scratch_deallocate_frame` calls,
  but `verify_bullet_proof` creates and destroys a scratch space per call.
  Carry Core's patch if the crate is ever vendored.

## Checklist by area

Crypto and proofs
- [x] Hand-declared schnorrsig `extern "C"` block matches grin 0.7.15's header (argument order, pointer types, 64-byte opaque signature); ABI vectors pass.
- [x] Output, input and kernel signatures accepted by Core 0.21.5.8 for every feature combination LDK authors (node suites).
- [x] `core_bulletproof_gate` passes on 0.21.5.8.
- [x] Scratch-frame leak assessed (P-10).
- [ ] Port `FromHash` fallback with known-answer vectors (P-8).

Parsing and serialization
- [x] Strictness table: every field rust-litecoin validates at decode compared with Core consensus. One is stricter than consensus (P-1, embargoed; table in the private report).
- [x] Kernel codec matches a model of Core's `Kernel::Serialize` for all 64 canonical feature combinations (`kernel_codec_parity`, fuzz `kernel_codec`).
- [x] Kernel ID divergence for non-canonical kernels pinned (P-9).
- [x] Extract canonicalization and casts pinned (P-5).
- [~] Kernel `lock_height` accepted up to `u32::MAX`; Core reads `int32`. Harmless for decode (Core cannot produce one), relevant to extract (P-5).
- [x] Historical census of mainnet blocks (heights 2,265,984 to 3,205,000; 939,017 MWEB blocks): 0 decode failures with the strict `Block` decoder, 0 non-canonical or non-standard kernels. P-9 has no mainnet instances.

Peg-in, peg-out, spend
- [x] Conservation property test: commitment and stealth sums balance for randomized spends with peg-outs (`tests/builder_conservation.rs`); verified non-vacuous by mutation.
- [ ] Policy guardrails (P-2).
- [x] LDK never emits the soft-forked encodings from its own builders.
- [ ] Transparent peg-out maturity (P-3).
- [x] Signer trust model analysed (P-7).

Receive scanning
- [x] Hostile outputs return "not ours" per output rather than abort a batch (fixed with P-1, unreleased).
- [x] Step order matches Core `RewindOutput`: view tag, B_i lookup, commitment, Ke re-derivation.
- [x] `MAX_MONEY` fail-closed check is reachable only for outputs that are not on chain, so under `Anchored` it cannot become a batch-level DoS.

Coin database
- [ ] Frozen-output exclusion (P-4).
- [ ] Pending-spend lifecycle and rollback (P-6).
- [ ] Reload revalidation (P-12).
- [ ] Reorg (`invalidateblock`) across a peg-in and a peg-out asserting value conservation (regtest scenario not yet written).

Sync integrity
- [~] Decoder now accepts everything consensus accepts for MWEB outputs and blocks (P-1, R-1; unreleased), so a decode failure is again evidence against the peer. Not banning on anchored-data decode failures, as defense in depth, is a separate peer-scoring change.
- [x] F-21 throttling still behaves on 0.21.5.8 (`lip0006_throttle`).
- [ ] Header source on 0.21.5.7+ and stale-tip detection.

Key derivation
- [x] `core_seed_parity` passes on 0.21.5.8.

Wallet facade
- [ ] `CombinedBalance::trusted_spendable` includes immature peg-ins (P-11).
- [ ] Caller-supplied fee validated (P-2).

Operational
- [ ] Update DNS seed (P-13).
- [ ] Broadcast via 0.21.5.8+ and confirm by wtxid.
- [ ] `bitcoind_rpc` emitter behaviour under `rpcserialversion<=1`.
- [ ] `SECURITY.md` routes reports to upstream `bitcoindevkit`; LDK needs its own channel before disclosure.

## New tests and tools in this pass

| Path | Kind |
| --- | --- |
| `crates/testenv/src/litecoin_regtest.rs` | `RelayPolicy`, `LITECOIND_RELAY_POLICY` |
| `crates/mweb/tests/core_policy_guardrails.rs` | Core 0.21.5.8 behaviour for fee floor, OP_RETURN and non-standard peg-outs, weight cap (both modes) |
| `crates/mweb/tests/policy_guardrails.rs` | P-2 red tests |
| `crates/mweb/tests/kernel_codec_parity.rs` | codec parity plus P-5 and P-9 red tests |
| `crates/mweb/tests/coin_db_parity.rs` | P-4, P-11, P-12 red tests |
| `crates/mweb/tests/builder_conservation.rs` | balance property test |
| `crates/mweb/src/mweb_sync.rs` (test module) | P-6 red test |
| `crates/chain/tests/test_canonical_view.rs` | P-3 red test |
| `crates/mweb/fuzz/fuzz_targets/{kernel_codec,psbt_extract}.rs` | new fuzz targets (`audit-red` feature for P-5) |
| `crates/mweb/examples/scan_mweb_utxo_keys.rs` | mainnet UTXO-set census over LIP-0006 (embargoed until P-1 ships) |
| `crates/mweb/examples/scan_mweb_blocks_decode.rs` | mainnet block decode and kernel census over RPC |

Run the red tests:

```bash
cd crates/mweb
cargo test --all-features --test policy_guardrails --test kernel_codec_parity --test coin_db_parity -- --ignored
cargo test --all-features --lib differential_sync_restores -- --ignored
(cd ../chain && cargo test --all-features --test test_canonical_view hogex -- --ignored)
(cd fuzz && cargo test --features audit-red --bin psbt_extract)
```

## Embargo

Until P-1, R-1, R-2 and R-3 are fixed and released, keep these out of any
public branch: the P-1 regtest test and its fuzz seeds (listed in the private
report), and the private report itself. Coordinate disclosure with Litecoin Core and ltcsuite
if the ltcd/mwebd decoders share the behaviour.
