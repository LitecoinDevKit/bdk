# Peg-in/out: maturity 6, v9 program = kernel id, amounts ≤ MAX_MONEY

## Decision

A peg-in is not selectable until `tip + 1 - height >= 6` (`MWEB_PEGIN_MATURITY`), and until then its value is reported as `MwebBalance::immature`, not `confirmed`. A transparent HogEx peg-out output is likewise immature until `tip + 1 - height >= 6` (`PEGOUT_MATURITY`, `CanonicalTxOut::is_mature`). The transparent v9 witness program must equal the peg-in kernel id. Authoring sums and kernel i64 casts reject anything above `MAX_MONEY` (84M LTC). Other MWEB coins are spendable once confirmed.

## Why

The two six-block rules have different sources:

- **Peg-out (transparent side): consensus.** Litecoin Core's `PEGOUT_MATURITY = 6` (`consensus/consensus.h`) is enforced in `Consensus::CheckTxInputs` and in the mempool: a HogEx output after the v8 HogAddr cannot be spent until the spending block is 6 above the HogEx. Core's wallet reports such outputs as immature (`CWalletTx::GetBlocksToMaturity`). Spending one earlier is rejected (`bad-txns-premature-spend-of-pegout`).
- **Peg-in (MWEB side): LDK wallet policy.** Core has no peg-in maturity rule; a peg-in's MWEB output is consensus-spendable once confirmed. LDK waits 6 blocks anyway, mirroring the peg-out window, so a shallow reorg that drops the peg-in cannot take a coin the wallet has already spent onward.

`mweb_pegin_script_pubkey`: program is the peg-in kernel id. `money_to_i64` / `checked_amount_total`: consensus `MoneyRange`; an `as i64` wrap would be a consensus mismatch. `rewind_output`: a value above supply can only be fabricated, so fail closed.

## How we'd know it broke

- `immature_pegin_not_spendable_until_maturity` / `non_pegin_spendable_when_confirmed` in `crates/mweb/src/coin_db.rs` (height 10: empty at tip 14, one coin at 15).
- `immature_pegin_is_not_in_confirmed_bucket` in `crates/mweb/tests/coin_db_parity.rs`.
- `test_hogex_pegout_is_immature_for_six_blocks` in `crates/chain/tests/test_canonical_view.rs`.
- `core_finalize_pegin_recognized_after_maturity` in `crates/testenv/tests/mweb_pegin.rs` — v9 `0x59 0x20 || kernel`; Core credits after maturity.
- `fixture_kernel_id_matches_v9_program` in `crates/mweb/src/tx_builder.rs` — `docs/mweb_pegin_regtest.hex` kernel id equals the v9 program.
- `fund_sign_pegin_maps_first_kernel_id` in `crates/mweb/src/psbt.rs`.
- `amount_totals_reject_overflow_and_supply_violation` / `kernel_amount_casts_are_bounded_by_max_money` in `crates/mweb/src/tx_builder.rs`.
- Node suites `crates/mweb/tests/core_spend.rs` and `core_pegin_pegout_roundtrip.rs` mine `MWEB_PEGIN_MATURITY` before spending.
