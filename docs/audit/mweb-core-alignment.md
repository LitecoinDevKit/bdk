# MWEB audit: LDK vs Litecoin Core 0.21.5.8

Scope: the MWEB funds path in `bdk_mweb`, the pinned `rust-litecoin` codecs and
PSBT extract (`5a0233712bdefc568fb45d71f42c028e196c4352`), and the `bdk_wallet`
facade, checked against Litecoin Core 0.21.5.5 through 0.21.5.8.

All fixes are released: rust-litecoin `90fc904` (LitecoinDevKit
`mweb-psbt-typed-maps`) and this branch, which pins it.

Status key: `[x]` done, `[ ]` open, `[~]` partially done.

## Summary

- **What was checked.** Every 2026 Core MWEB fix (0.21.5.5 to 0.21.5.8) mapped
  to LDK; an independent review of the parsing and crypto code; LDK-authored
  spends, peg-ins and peg-outs against a Core 0.21.5.8 node under both strict
  and permissive relay policy; the full mainnet MWEB UTXO set (59,344 outputs)
  and every MWEB-era mainnet block (939,017) decoded with LDK's decoders.
- **What was found.** 13 audit findings (P-1 to P-13) and 9 from the
  independent review (R-1 to R-9). Two are High (P-1, R-1): consensus-valid
  data that LDK's decoders rejected, so one mined block could stall every LDK
  client. Both reproduced on regtest; neither has ever occurred on mainnet. The
  worst funds issue let a caller author an OP_RETURN peg-out that relays and
  burns its value (P-2).
- **What is fixed.** Everything except R-5 (deferred, below). The rust-litecoin
  fixes (P-1, R-1, R-2, P-5, R-4, R-6, R-9) are in `90fc904`.

## Residual risk after these fixes

- **Omission by an anchored peer.** A peer that serves only valid, anchored data
  can still leave outputs out of a differential pass; a full pass or a second
  peer catches it.
- **No mempool watch.** Incoming unconfirmed MWEB payments are invisible until
  mined.
- **`Trusted` sync mode** remains public API; use `Anchored`.
- **Plaintext persistence by default.** Enable `encrypt-changeset`.
- **PSBT coordinator trust.** `sign_funded_mweb` signs what the wallet funded,
  but a coordinator-supplied PSBT's outputs and kernel are not validated by
  `sign_mweb_components` (which now refuses to sign at all).
- **Unconfirmed spends.** A coin marked spent at broadcast is restored after 12
  blocks if its leaf is still unspent; a spend that confirms after that makes
  the next pass mark it spent again.
- **R-5.** The txid rust-litecoin computes for an MWEB-only transaction is not
  Core's (the first kernel's BLAKE3 hash); all such transactions share one txid.
  Use `bdk_mweb::kernel_id` on the first kernel to identify them. Fixing it in
  rust-litecoin needs a BLAKE3 dependency, which its MSRV (1.56.1) rules out.

Recommended environment: Litecoin Core 0.21.5.8 or later for LIP-0006 peers,
the header source and broadcast (older nodes enforce neither soft fork and can
censor MWEB transactions through recentRejects); `noban` whitelisting for
trusted peers; `VerifyMode::Anchored`; `encrypt-changeset`.

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
- [x] All node-backed suites pass on 0.21.5.8 in both modes (bulletproof gate,
      receive scan, seed parity, spend, peg-in/peg-out roundtrip, relay-policy
      guardrails, LIP-0006 sync/p2p/throttle, anchoring). Spends, peg-ins and
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
| 0.21.5.5 | `773c138f78` `SecretKey::FromHash` fallback | ported (`hash::secret_from_hash`) | P-8 |
| 0.21.5.5 | `1cd94bbcb6` amount hardening, MWEB fee = weight x 100 | fee floor enforced (`bdk_mweb::policy`) | P-2 |
| 0.21.5.5 | `8f8ad64d04` duplicate peg-in guards | LDK authors one kernel per peg-in | checked |
| 0.21.5.5 | `a549004385` kernel lock height enforced | LDK never sets one; extract bounds it | P-5 |
| 0.21.5.5 | `c25bf8923c` peg-out scripts must be standard | wallet script types only, non-dust | P-2 |
| 0.21.5.5 | frozen MWEB output IDs | excluded from balance and selection | P-4 |
| 0.21.5.6 | `cfdfcf57aa` output Ko/Ke validity (relay policy and wallet only, not consensus) | decoder rejected off-curve Ko/Ke; now raw bytes, checked lazily | P-1 |
| 0.21.5.6 | `b250b016a9` / `67580d6e35` empty-pegout soft fork | extract rejects it; kernel ID follows Core | P-5, P-9 |
| 0.21.5.6 | `109ed13b73` / `dc72334202` 4,000 weight / 1,000 input relay cap | enforced | P-2 |
| 0.21.5.6 | `4dd6aeeb97` / `136695e050` mutated blocks | node-side; covered by anchoring | checked |
| 0.21.5.7 | `a399fc4ca7` / `b4313c34d2` empty-extra-data soft fork, revalidate connected body | extract rejects it; kernel ID follows Core; coins revalidated on load | P-5, P-9, P-12 |
| 0.21.5.7 | `d3b9ac3969` MWEB-only mempool txs hidden at `rpcserialversion<=1` | not relevant: the RPC emitter tracks transparent txs, and MWEB-only txs have none | checked by reading |
| 0.21.5.8 | `d7ab03d31c` bulletproof scratch-frame leak | present, cannot accumulate | P-10 |
| 0.21.5.8 | `c07d6228c0` recentRejects censorship of MWEB txs | broadcast via 0.21.5.8+ | operational |
| 0.21.5.8 | `f6afd26826` / `70ea696f36` / `0cb1419649` / `36f06455f9` | node-side | P-6 |
| 0.21.5.8 | `63ba41f50e` DNS seed replaced | updated | P-13 |

## Findings

| ID | Severity | Status | Fix | Regression test |
| --- | --- | --- | --- | --- |
| P-1 | High | Fixed | rust-litecoin decoded output Ko and Ke as curve points; consensus does not require it. One mined off-curve output made every `mwebutxos` batch carrying it undecodable (classed as banworthy, so honest peers were banned and full sync never finished) and its block undecodable for `bdk_bitcoind_rpc`; an off-curve Ko can never be spent, so it would stay forever. Ko/Ke are now raw `[u8; 33]` with checked accessors, and `rewind_output` treats them as not ours | `core_offcurve_output.rs` (regtest, Core 0.21.5.8), `scan::tests::off_curve_ko_or_ke_is_not_ours_and_does_not_abort_the_batch`, rust-litecoin `output_with_off_curve_ko_and_ke_decodes` |
| P-2 | Medium | Fixed | `bdk_mweb::policy`: fee ≥ weight x 100, ≤ 4,000 weight, ≤ 1,000 inputs, peg-outs only to P2PKH/P2SH/P2WPKH/P2WSH/P2TR above dust; `MwebTxBuilder::skip_relay_policy_checks` for node-policy tests | `tests/policy_guardrails.rs`, `tests/core_policy_guardrails.rs` |
| P-3 | Low | Fixed | `CanonicalTxOut::is_hogex_pegout`; `is_mature` applies `PEGOUT_MATURITY` | `test_canonical_view.rs::test_hogex_pegout_is_immature_for_six_blocks` |
| P-4 | Low | Fixed | `FROZEN_MWEB_OUTPUT_IDS` excluded from balance and selection | `tests/coin_db_parity.rs` |
| P-5 | Low | Fixed | extract derives or validates feature bytes, checked casts, exact field lengths | `tests/kernel_codec_parity.rs`, fuzz `psbt_extract --features audit-red` |
| P-6 | Medium | Fixed | sync restores a locally spent coin whose leaf stays unspent for `UNCONFIRMED_SPEND_GRACE_BLOCKS` (12); `ChangeSet::restored` persists the undo | `mweb_sync::tests::differential_sync_restores_coin_whose_spend_never_confirmed`, `changeset::tests::merge_restored_undoes_an_earlier_spend` |
| P-7 | Low | Fixed | `sign_mweb_components` scrubs input secrets | `psbt::tests::finished_mweb_tx_maps_roundtrip_and_extract` |
| P-8 | Low | Fixed | `hash::secret_from_hash` at every point Core uses `FromHash` | `hash::tests::secret_from_hash_matches_core` |
| P-9 | Low | Fixed; no mainnet instances | `kernel_id` hashes Core's presence-driven serialization | `tests/kernel_codec_parity.rs::noncanonical_kernel_id_matches_core` |
| P-10 | Info | Accepted | carry Core's patch if grin is vendored | — |
| P-11 | Low | Fixed | `MwebBalance::immature` | `tests/coin_db_parity.rs` |
| P-12 | Low | Fixed | commitments re-checked on load; `rejected_on_load` | `tests/coin_db_parity.rs` |
| P-13 | Low | Fixed | `dnsseed.ltcpool.org` | — |
| R-1 | High | Fixed | rust-litecoin read a block's MWEB extension only when header version bit 29 was set; Core reads it whenever the block ends in a HogEx, and any version ≥ 0x20000000 is valid, so a miner could stall `bdk_bitcoind_rpc` with a valid block. Now follows Core | rust-litecoin `hogex_block_without_version_bit_29_decodes`; reproduced on regtest with `-blockversion` |
| R-2 | Medium | Fixed | the index in a global MWEB PSBT key sized an allocation (`0xffffffff` exhausted memory); now bounded by consensus maxima | rust-litecoin `decode_rejects_mweb_map_index_beyond_consensus_limits` |
| R-3 | Medium | Fixed | `psbt_from_ltcd_v2` reserved memory from untrusted section counts; now bounded | `ltcd_psbt_fixtures::ltcd_v2_rejects_untrusted_section_counts` |
| R-4 | Medium | Fixed | `compute_wtxid` follows Core for every block transaction | rust-litecoin `hogex_block_witness_commitment_verifies` |
| R-5 | Low | Open | needs BLAKE3 in rust-litecoin (MSRV) | — |
| R-6 | Low | Fixed | PSBT `combine` pairs MWEB maps by id, commitment, excess | rust-litecoin `combine_pairs_mweb_inputs_by_output_id` |
| R-7 | Low | Fixed | `sign_mweb_components` refuses unsigned inputs | `psbt::tests::finished_mweb_tx_maps_roundtrip_and_extract` |
| R-8 | Low | Fixed | redacted `Debug`; `Zeroizing` in `sign_funded_mweb` | — |
| R-9 | Info | Fixed | reject superfluous witness flag with MWEB and presence bytes 2-255 | rust-litecoin `rejects_superfluous_witness_flag_and_bad_mweb_presence_byte` |

Notes:

- **P-2.** Core 0.21.5.8 accepts a fee of exactly weight x 100 and rejects one
  litoshi less, in both relay modes. It relays an OP_RETURN peg-out even under
  strict policy (`NULL_DATA` is standard and exempt from dust). Non-standard
  peg-out scripts and transactions over 4,000 weight are refused only in strict
  mode.
- **P-5.** A PSBT kernel fee above `i64::MAX` reached the wire as 1 litoshi.
  Kernel signatures commit to the fee, so the transaction was rejected rather
  than mispaying. LDK's own fund-then-sign path was never affected. An ltcd
  test fixture declaring input features 0 alongside an input pubkey is now
  refused, as a bit-driven encoder would drop that key.
- **P-7.** The input signatures `sign_mweb_components` produced could not be
  reused by a coordinator: each used a fresh ephemeral key whose `ki - ko` term
  the stealth sum needs. R-7 is the other side of that: nothing it signed could
  balance. It no longer signs.

## Checklist by area

Crypto and proofs
- [x] Hand-declared schnorrsig `extern "C"` block matches grin 0.7.15's header; ABI vectors pass.
- [x] Output, input and kernel signatures accepted by Core 0.21.5.8 for every feature combination LDK authors.
- [x] `core_bulletproof_gate` passes on 0.21.5.8.
- [x] Scratch-frame leak assessed (P-10).
- [x] `FromHash` fallback ported (P-8).

Parsing and serialization
- [x] Strictness table: of the keys rust-litecoin validated at decode, only output Ko and Ke were stricter than Core consensus (P-1); Ks, input keys and the kernel stealth excess are required valid by Core's signature and stealth-sum checks. Mainnet UTXO set at 3,205,231: 0 of 59,344 outputs have an off-curve key.
- [x] Kernel codec matches Core's `Kernel::Serialize` for all canonical feature combinations (`kernel_codec_parity`, fuzz `kernel_codec`).
- [x] Kernel ID follows Core for non-canonical kernels (P-9).
- [x] Extract canonicalizes or rejects (P-5).
- [x] Historical census of mainnet blocks (heights 2,265,984 to 3,205,000; 939,017 MWEB blocks): 0 decode failures with the strict `Block` decoder, 0 non-canonical or non-standard kernels.
- [ ] MWEB-only txid (R-5).

Peg-in, peg-out, spend
- [x] Conservation property test: commitment and stealth sums balance for randomized spends with peg-outs (`tests/builder_conservation.rs`); verified non-vacuous by mutation.
- [x] Relay-policy guardrails (P-2).
- [x] LDK never emits the soft-forked encodings from its own builders.
- [x] Transparent peg-out maturity (P-3).
- [x] Signer trust model (P-7, R-7).

Receive scanning
- [x] Hostile outputs return "not ours" per output rather than abort a batch (P-1).
- [x] Step order matches Core `RewindOutput`.
- [x] `MAX_MONEY` fail-closed check is reachable only for outputs that are not on chain.

Coin database
- [x] Frozen-output exclusion (P-4).
- [x] Pending-spend rollback (P-6).
- [x] Reload revalidation (P-12).
- [ ] Reorg (`invalidateblock`) across a peg-in and a peg-out asserting value conservation (regtest scenario not yet written).

Sync integrity
- [~] Decoders accept everything consensus accepts (P-1, R-1), so a decode failure is again evidence against the peer. Not banning on anchored-data decode failures, as defense in depth, is a separate peer-scoring change.
- [x] F-21 throttling still behaves on 0.21.5.8 (`lip0006_throttle`).
- [ ] Header source on 0.21.5.7+ and stale-tip detection.

Key derivation
- [x] `core_seed_parity` passes on 0.21.5.8.

Wallet facade (`bdk_wallet`, separate repository)
- [~] `CombinedBalance::trusted_spendable` excludes immature peg-ins once bdk_wallet is on this bdk_mweb (P-11; `mweb_confirmed` is now spendable-only). `CombinedBalance` must also carry `MwebBalance::immature`, or its `total()` will omit young peg-ins after the bump.
- [x] Caller-supplied fee validated by `fund_mweb_spend` (P-2).
- [ ] `Wallet::sign_mweb_components` still passes spent coins through; harmless now that the call no longer signs, but worth removing.

Operational
- [x] DNS seed updated (P-13).
- [ ] Broadcast via 0.21.5.8+ and confirm by txid/wtxid.
- [x] `SECURITY.md` (here and in rust-litecoin) routes reports to GitHub private vulnerability reporting on LitecoinDevKit.

Pre-existing gaps noticed, not from this audit
- [ ] `cargo test -p bdk_mweb --no-default-features --features std,persist` has 9 failing tests that need `zkp` but are not gated on it.
- [ ] rust-litecoin's `serde` integration test does not compile (a `Psbt` literal is missing the MWEB fields).

## Tests and tools added

| Path | Kind |
| --- | --- |
| `crates/testenv/src/litecoin_regtest.rs` | `RelayPolicy`, `LITECOIND_RELAY_POLICY` |
| `crates/mweb/tests/core_policy_guardrails.rs` | Core 0.21.5.8 behaviour for fee floor, OP_RETURN and non-standard peg-outs, weight cap (both modes) |
| `crates/mweb/tests/policy_guardrails.rs` | P-2 |
| `crates/mweb/tests/kernel_codec_parity.rs` | codec parity, P-5, P-9 |
| `crates/mweb/tests/coin_db_parity.rs` | P-4, P-11, P-12 |
| `crates/mweb/tests/builder_conservation.rs` | balance property test |
| `crates/mweb/src/policy.rs`, `hash.rs`, `mweb_sync.rs`, `changeset.rs` (test modules) | P-2, P-8, P-6 |
| `crates/chain/tests/test_canonical_view.rs` | P-3 |
| `crates/mweb/fuzz/fuzz_targets/{kernel_codec,psbt_extract}.rs` | new fuzz targets (`audit-red` asserts P-5's fixed behaviour) |
| `crates/mweb/tests/core_offcurve_output.rs` | P-1 regression against Core 0.21.5.8 |
| `crates/mweb/examples/scan_mweb_utxo_keys.rs` | mainnet MWEB UTXO-set key and frozen-output census over LIP-0006 |
| `crates/mweb/examples/scan_mweb_blocks_decode.rs` | mainnet block decode and kernel census over RPC |
