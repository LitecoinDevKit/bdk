//! Litecoin Core 0.21.5.8 relay policy for the MWEB transactions LDK authors.
//!
//! Consensus accepts far more than a node will relay. A transaction that fails
//! these checks is either refused by every node (it never confirms) or, for an
//! OP_RETURN peg-out, relays and destroys the value. The constants and formulas
//! follow `mw::Weight`, `mw/consensus/Params.h`, `policy/feerate.cpp`,
//! `mweb/mweb_policy.cpp` and `policy/policy.cpp`.

use alloc::format;

use bitcoin::{Script, WitnessVersion};

use crate::error::Error;

/// Litoshis of fee per unit of MWEB weight (Core `BASE_MWEB_FEE`).
pub const BASE_MWEB_FEE: u64 = 100;
/// Bytes of extra data (or peg-out script) per unit of weight (`mw::BYTES_PER_WEIGHT`).
pub const BYTES_PER_WEIGHT: u64 = 42;
/// Weight of a kernel carrying a stealth excess, as LDK's always do.
pub const KERNEL_WITH_STEALTH_WEIGHT: u64 = 3;
/// Weight of an output with standard fields, as LDK's always have.
pub const STANDARD_OUTPUT_WEIGHT: u64 = 18;
/// Largest relayed MWEB transaction (`MAX_STANDARD_MWEB_TX_WEIGHT`).
pub const MAX_STANDARD_MWEB_TX_WEIGHT: u64 = 4_000;
/// Most inputs in a relayed MWEB transaction (`MAX_STANDARD_MWEB_TX_INPUTS`).
pub const MAX_STANDARD_MWEB_TX_INPUTS: usize = 1_000;
/// Litecoin's default dust relay fee, litoshis per 1,000 bytes (`DUST_RELAY_TX_FEE`).
pub const DUST_RELAY_FEE_PER_KB: u64 = 30_000;

/// MWEB weight of a transaction LDK authors: one stealth kernel carrying
/// `pegout_scripts`, `outputs` standard outputs, and inputs without extra data
/// (which weigh nothing).
pub fn mweb_weight<'a>(
    outputs: usize,
    pegout_scripts: impl IntoIterator<Item = &'a Script>,
) -> u64 {
    let pegouts: u64 = pegout_scripts
        .into_iter()
        .map(|s| (s.len() as u64).div_ceil(BYTES_PER_WEIGHT))
        .sum();
    KERNEL_WITH_STEALTH_WEIGHT + pegouts + outputs as u64 * STANDARD_OUTPUT_WEIGHT
}

/// The fee a node requires for `weight` (Core `GetMWEBFeeForWeight`).
pub fn min_mweb_fee(weight: u64) -> u64 {
    weight.saturating_mul(BASE_MWEB_FEE)
}

/// Core `GetDustThreshold` at the default dust relay fee.
pub fn dust_threshold(script: &Script) -> u64 {
    let len = script.len() as u64;
    let txout = 8 + bitcoin::VarInt(len).size() as u64 + len;
    let spend = if script.is_witness_program() {
        32 + 4 + 1 + 107 / 4 + 4
    } else {
        32 + 4 + 1 + 107 + 4
    };
    (txout + spend) * DUST_RELAY_FEE_PER_KB / 1_000
}

/// Refuse a peg-out a node would not relay, or that would destroy its value.
///
/// Only the script types a wallet sends to are accepted: P2PKH, P2SH, P2WPKH,
/// P2WSH and P2TR. That rules out empty and non-standard scripts (refused by
/// relay policy), OP_RETURN (relayed, and the value is burned), and the
/// witness v8/v9 programs MWEB itself uses for the HogEx and peg-ins.
pub fn check_pegout(script: &Script, amount: u64) -> Result<(), Error> {
    if script.is_op_return() {
        return Err(Error::Policy(
            "peg-out to OP_RETURN would burn the amount".into(),
        ));
    }
    if matches!(
        script.witness_version(),
        Some(WitnessVersion::V8 | WitnessVersion::V9)
    ) {
        return Err(Error::Policy(
            "peg-out to an MWEB HogAddr or peg-in script".into(),
        ));
    }
    let wallet_type = script.is_p2pkh()
        || script.is_p2sh()
        || script.is_p2wpkh()
        || script.is_p2wsh()
        || script.is_p2tr();
    if !wallet_type {
        return Err(Error::Policy(format!(
            "peg-out script is not a standard address type: {script}"
        )));
    }
    let dust = dust_threshold(script);
    if amount < dust {
        return Err(Error::Policy(format!(
            "peg-out of {amount} is below the {dust} dust threshold"
        )));
    }
    Ok(())
}

/// Check an MWEB spend's shape and fee against relay policy.
///
/// `outputs` counts every output including change.
pub fn check_spend<'a>(
    inputs: usize,
    outputs: usize,
    pegout_scripts: impl IntoIterator<Item = &'a Script>,
    fee: u64,
) -> Result<(), Error> {
    if inputs > MAX_STANDARD_MWEB_TX_INPUTS {
        return Err(Error::Policy(format!(
            "{inputs} MWEB inputs exceed the relay limit of {MAX_STANDARD_MWEB_TX_INPUTS}"
        )));
    }
    let weight = mweb_weight(outputs, pegout_scripts);
    if weight > MAX_STANDARD_MWEB_TX_WEIGHT {
        return Err(Error::Policy(format!(
            "MWEB weight {weight} exceeds the relay limit of {MAX_STANDARD_MWEB_TX_WEIGHT}"
        )));
    }
    check_fee(weight, fee)
}

/// [`check_pegout`] for every peg-out, then [`check_spend`]: everything LDK
/// checks before authoring a spend or peg-out.
pub fn check_authored(
    inputs: usize,
    outputs: usize,
    pegouts: &[(bitcoin::ScriptBuf, u64)],
    fee: u64,
) -> Result<(), Error> {
    for (script, amount) in pegouts {
        check_pegout(script, *amount)?;
    }
    check_spend(
        inputs,
        outputs,
        pegouts.iter().map(|(s, _)| s.as_script()),
        fee,
    )
}

/// Refuse a fee below the MWEB minimum for `weight`.
pub fn check_fee(weight: u64, fee: u64) -> Result<(), Error> {
    let min = min_mweb_fee(weight);
    if fee < min {
        return Err(Error::Policy(format!(
            "fee {fee} is below the MWEB minimum of {min} for weight {weight}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::ScriptBuf;

    fn p2wpkh() -> ScriptBuf {
        ScriptBuf::from_bytes([0x00, 0x14].into_iter().chain([7u8; 20]).collect())
    }

    fn p2pkh() -> ScriptBuf {
        let mut v = vec![0x76, 0xa9, 0x14];
        v.extend([7u8; 20]);
        v.extend([0x88, 0xac]);
        ScriptBuf::from_bytes(v)
    }

    #[test]
    fn weight_and_fee_match_core() {
        // One recipient + change + stealth kernel.
        assert_eq!(mweb_weight(2, []), 39);
        assert_eq!(min_mweb_fee(39), 3_900);
        // A 22-byte peg-out script adds one unit.
        assert_eq!(mweb_weight(1, [p2wpkh().as_script()]), 22);
    }

    #[test]
    fn dust_thresholds_match_core_at_30_000_per_kb() {
        assert_eq!(dust_threshold(&p2wpkh()), 2_940);
        assert_eq!(dust_threshold(&p2pkh()), 5_460);
        assert!(check_pegout(&p2wpkh(), 2_939).is_err());
        assert!(check_pegout(&p2wpkh(), 2_940).is_ok());
    }
}
