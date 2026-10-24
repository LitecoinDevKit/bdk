# Scan: rewind stays on-device and will not accept a lying value

## Decision

Scan/spend keys never leave the process for LIP-0006 sync. `rewind_output` recomputes `switch_commit(pre_blind, value)` and returns `None` on commitment mismatch; it also rejects a rewound value `> MAX_MONEY`. Rangeproofs/signatures are not checked on the default sync path; that trust is PMMR inclusion under an `Anchored` root. `Trusted` / `HeaderAndPmmr` require `MwebSyncer::verify_rangeproofs` or an external cross-check.

## Why

`crates/mweb/src/scan.rs` trust-model comment: rewind does not verify proofs; it does stop a fabricated output from *misreporting* value; a peer that authors the output whole can still choose any supply-bounded value. `docs/MWEB_ARCHITECTURE.md`: LIP-0006-style scan locally (scan key stays on-device). `pmmr` module docs: inclusion-under-anchored-root is what substitutes for proof verification, and it is void in `Trusted`.

## How we'd know it broke

- `ltcd_rewind_output_vector` / `ltcd_rewind_wrong_scan_key_fails` in `crates/mweb/src/scan.rs` — wrong scan key → `None`; fixture output rewinds to the expected amount.
- Commitment check at `scan.rs` (`expected_commit != output.commitment` → `Ok(None)`). Fuzz target `crates/mweb/fuzz/fuzz_targets/rewind_output.rs` — must never return a coin for a non-matching output.
- `MAX_MONEY` fail-closed in `rewind_output` — no dedicated unit test today; `amount_totals_reject_overflow_and_supply_violation` covers the authoring side; `balance_saturates_on_fabricated_amounts` in `coin_db.rs` covers a lying peer that already got a coin stored.
- `core_receive_scan` / `lip0006_sync` node tests — owned peg-in is found only with the local `MasterKeys`.
