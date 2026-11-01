//! P-2: LDK authoring guardrails against Litecoin Core 0.21.5.8 relay policy.
//!
//! `core_policy_guardrails` shows what Core does with each of these; here we
//! assert that `fund_mweb_spend` refuses them up front. All are red today.

use bdk_mweb::keys::{MasterKeyScheme, MasterKeys};
use bdk_mweb::psbt_fund::fund_mweb_spend;
use bdk_mweb::MwebCoin;
use bitcoin::key::Secp256k1;
use bitcoin::opcodes::all::OP_PUSHNUM_1;
use bitcoin::script::Builder;
use bitcoin::{Network, NetworkKind, ScriptBuf, WitnessProgram, WitnessVersion};

const SEED: [u8; 32] = [7; 32];
const AMOUNT: u64 = 100_000_000;

fn keys() -> MasterKeys {
    MasterKeys::from_seed(
        &SEED,
        Network::Regtest,
        MasterKeyScheme::LitecoinCore,
        &Secp256k1::new(),
    )
    .unwrap()
}

/// Funding only reads the coin's identity and amount; no secrets are needed.
fn coin() -> MwebCoin {
    MwebCoin {
        output_id: [1; 32],
        commitment: [8; 33],
        amount: AMOUNT,
        address_index: 2,
        blind: [2; 32],
        shared_secret: [3; 32],
        spend_key: Some([4; 32]),
        block_height: Some(500),
        is_pegin: false,
        leaf_index: None,
    }
}

fn fund(
    recipients: usize,
    pegouts: Vec<(ScriptBuf, u64)>,
    fee: u64,
) -> Result<(), bdk_mweb::Error> {
    let secp = Secp256k1::new();
    let k = keys();
    let to = (0..recipients)
        .map(|i| {
            (
                k.address(10 + i as u32, NetworkKind::Test, &secp).unwrap(),
                10_000,
            )
        })
        .collect();
    fund_mweb_spend(
        vec![coin()],
        to,
        pegouts,
        fee,
        &k,
        1,
        NetworkKind::Test,
        &secp,
    )
    .map(|_| ())
}

fn p2wpkh() -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&bitcoin::WPubkeyHash::from_raw_hash(
        bitcoin::hashes::Hash::from_byte_array([9; 20]),
    ))
}

#[test]
#[ignore = "P-2 red test: no MWEB weight or fee floor"]
fn refuses_fee_below_weight_times_100() {
    // One recipient + change + stealth kernel = weight 39, floor 3,900.
    assert!(fund(1, vec![], 3_900).is_ok());
    assert!(fund(1, vec![], 3_899).is_err());
}

#[test]
#[ignore = "P-2 red test: no 4,000-weight relay cap"]
fn refuses_tx_over_relay_weight_cap() {
    // 222 recipients + change + stealth kernel = weight 4,017.
    assert!(fund(222, vec![], 4_017 * 100).is_err());
}

#[test]
#[ignore = "P-2 red test: peg-out scripts are not checked"]
fn refuses_op_return_pegout() {
    let burn = ScriptBuf::new_op_return([0u8; 0]);
    assert!(fund(0, vec![(burn, 10_000_000)], 50_000).is_err());
}

#[test]
#[ignore = "P-2 red test: peg-out scripts are not checked"]
fn refuses_nonstandard_empty_and_mweb_pegout_scripts() {
    let anyone = Builder::new().push_opcode(OP_PUSHNUM_1).into_script();
    let pegin = ScriptBuf::new_witness_program(
        &WitnessProgram::new(WitnessVersion::V9, &[0u8; 32]).unwrap(),
    );
    let hogaddr = ScriptBuf::new_witness_program(
        &WitnessProgram::new(WitnessVersion::V8, &[0u8; 32]).unwrap(),
    );
    for spk in [anyone, ScriptBuf::new(), pegin, hogaddr] {
        assert!(
            fund(0, vec![(spk.clone(), 10_000_000)], 50_000).is_err(),
            "{spk:?}"
        );
    }
}

#[test]
#[ignore = "P-2 red test: peg-out dust is not checked"]
fn refuses_dust_pegout() {
    assert!(fund(0, vec![(p2wpkh(), 1)], 50_000).is_err());
}
