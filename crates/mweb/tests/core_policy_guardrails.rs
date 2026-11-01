//! What Litecoin Core's relay policy does with MWEB spends LDK can author today.
//!
//! These pin Core's behaviour, which `bdk_mweb::policy` mirrors: they build
//! with the builder's relay-policy checks switched off and show what a node
//! does with each case. Run under both `LITECOIND_RELAY_POLICY` values; the
//! expected outcome differs only where Core's MWEB relay policy applies.
//!
//! Needs `LITECOIND_EXE` (skips when unset).

use bdk_mweb::keys::{MasterKeyScheme, MasterKeys};
#[allow(deprecated)]
use bdk_mweb::tx_builder::{build_pegin, MwebTxBuilder, CHANGE_ADDRESS_INDEX};
use bdk_mweb::{scan_litecoin_tx, AddressBook, MwebCoin, MwebCoinDatabase, DEFAULT_GAP_LIMIT};
use bdk_testenv::{
    relay_policy, try_node_from_env, LitecoinNodeEnv, RelayPolicy, MWEB_PEGIN_MATURITY,
};
use bitcoin::key::Secp256k1;
use bitcoin::opcodes::all::OP_PUSHNUM_1;
use bitcoin::script::Builder;
use bitcoin::{Amount, Network, NetworkKind, ScriptBuf, Transaction};
use hex_conservative::FromHex;

const SEED_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

/// Core `BASE_MWEB_FEE` (`policy/feerate.cpp`): litoshis per unit of MWEB weight.
const BASE_MWEB_FEE: u64 = 100;
/// Core `mw::KERNEL_WITH_STEALTH_WEIGHT` and `mw::STANDARD_OUTPUT_WEIGHT`.
const KERNEL_WEIGHT: u64 = 3;
const OUTPUT_WEIGHT: u64 = 18;

fn setup() -> Option<(LitecoinNodeEnv, MasterKeys, MwebCoin)> {
    let env = try_node_from_env().expect("harness")?;
    let secp = Secp256k1::new();
    let seed = <Vec<u8>>::from_hex(SEED_HEX).unwrap();
    let keys = MasterKeys::from_seed(
        &seed,
        Network::Regtest,
        MasterKeyScheme::LitecoinCore,
        &secp,
    )
    .unwrap();
    let book = AddressBook::from_keys(&keys, DEFAULT_GAP_LIMIT, &secp).unwrap();
    let mining = env.mine_to_pre_mweb().unwrap();
    let pegin_amount = Amount::from_btc(1.0).unwrap().to_sat();
    #[allow(deprecated)]
    let pegin = build_pegin(&keys, 2, pegin_amount, 50_000, NetworkKind::Test, &secp).unwrap();
    let tx = env
        .broadcast_bdk_pegin(pegin.kernel_id, Amount::from_sat(pegin_amount), pegin.mw_tx)
        .unwrap();
    env.mine_mweb_activation(&mining).unwrap();
    env.mine_blocks(MWEB_PEGIN_MATURITY, &mining).unwrap();
    let mut db = MwebCoinDatabase::new();
    let coin = scan_litecoin_tx(&keys, &book, &tx, &mut db, &secp)
        .unwrap()
        .pop()
        .unwrap();
    Some((env, keys, coin))
}

fn build(
    keys: &MasterKeys,
    coin: &MwebCoin,
    recipients: usize,
    pegout: Option<(ScriptBuf, u64)>,
    fee: u64,
) -> Transaction {
    let secp = Secp256k1::new();
    // LDK now refuses all of these itself (`policy_guardrails`); skip that to
    // see what the node does with them.
    let mut b = MwebTxBuilder::new()
        .add_input(coin.clone())
        .fee(fee)
        .skip_relay_policy_checks();
    for i in 0..recipients {
        let addr = keys
            .address(10 + i as u32, NetworkKind::Test, &secp)
            .unwrap();
        b = b.add_recipient(addr, 10_000);
    }
    if let Some((spk, amt)) = pegout {
        b = b.add_pegout(spk, amt);
    }
    b.finish(keys, CHANGE_ADDRESS_INDEX, NetworkKind::Test, &secp)
        .unwrap()
        .tx
}

fn accept(env: &LitecoinNodeEnv, tx: &Transaction) -> Result<(), String> {
    match env.rpc.test_mempool_accept(tx).unwrap() {
        (true, _) => Ok(()),
        (false, reason) => Err(reason.unwrap_or_default()),
    }
}

/// The MWEB fee floor is exactly weight x 100 litoshi, in both relay modes.
/// LDK computes no weight, so a caller-chosen fee one litoshi short builds and
/// signs fine and is then refused by every node.
#[test]
fn mweb_fee_floor_is_weight_times_100() {
    let Some((env, keys, coin)) = setup() else {
        return;
    };
    // One recipient plus change, one stealth kernel.
    let floor = (KERNEL_WEIGHT + 2 * OUTPUT_WEIGHT) * BASE_MWEB_FEE;
    let short = accept(&env, &build(&keys, &coin, 1, None, floor - 1));
    assert!(
        short.as_ref().is_err_and(|r| r.contains("min fee not met")),
        "{short:?}"
    );
    assert_eq!(accept(&env, &build(&keys, &coin, 1, None, floor)), Ok(()));
}

/// An OP_RETURN peg-out is standard (`NULL_DATA`) and exempt from the dust
/// rule, so Core relays it under the strictest policy and the value is burned.
#[test]
fn op_return_pegout_relays_and_burns() {
    let Some((env, keys, coin)) = setup() else {
        return;
    };
    let burn = ScriptBuf::new_op_return([0u8; 0]);
    let tx = build(&keys, &coin, 0, Some((burn, 10_000_000)), 50_000);
    assert_eq!(
        accept(&env, &tx),
        Ok(()),
        "Core relays a 0.1 LTC OP_RETURN peg-out"
    );
}

/// A bare `OP_1` peg-out (anyone-can-spend) is consensus-valid; only strict
/// relay policy refuses it.
#[test]
fn nonstandard_pegout_script_needs_strict_policy() {
    let Some((env, keys, coin)) = setup() else {
        return;
    };
    let anyone = Builder::new().push_opcode(OP_PUSHNUM_1).into_script();
    let res = accept(
        &env,
        &build(&keys, &coin, 0, Some((anyone, 10_000_000)), 50_000),
    );
    match relay_policy() {
        RelayPolicy::Strict => assert_eq!(res, Err("scriptpubkey".into())),
        RelayPolicy::Permissive => assert_eq!(res, Ok(())),
    }
}

/// Core caps a relayed MWEB tx at 4,000 weight (`MAX_STANDARD_MWEB_TX_WEIGHT`).
/// 222 outputs plus change and one stealth kernel weigh 4,017.
#[test]
fn mweb_weight_cap_needs_strict_policy() {
    let Some((env, keys, coin)) = setup() else {
        return;
    };
    let outputs = 222;
    let weight = KERNEL_WEIGHT + (outputs as u64 + 1) * OUTPUT_WEIGHT;
    assert!(weight > 4_000);
    let res = accept(
        &env,
        &build(&keys, &coin, outputs, None, weight * BASE_MWEB_FEE),
    );
    match relay_policy() {
        RelayPolicy::Strict => assert_eq!(res, Err("mweb-txn-oversize".into())),
        RelayPolicy::Permissive => assert_eq!(res, Ok(())),
    }
}
