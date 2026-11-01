//! P-1: a consensus-valid MWEB output whose receiver key (Ko) and key-exchange
//! key (Ke) are not curve points.
//!
//! Core never checks Ko/Ke in consensus; since 0.21.5.6 only relay policy
//! (`Output::IsStandard`) rejects them. Once such an output is mined, both
//! decoder paths in LDK must still work: the raw block (the
//! `bdk_bitcoind_rpc` path) and the `mwebutxos` batch that carries it (the
//! LIP-0006 path), and the wallet's own outputs beside it must still be found.
//!
//! Needs `LITECOIND_EXE` (skips when unset). Under `LITECOIND_RELAY_POLICY=strict`
//! the node refuses to relay the output, which is all that mode can show:
//! `generateblock` cannot assemble a HogEx, so mining it needs permissive mode.

#![cfg(feature = "lip0006")]

use bdk_mweb::crypto::{
    blind_sum, blind_switch, bulletproof_prove, pedersen_commit, random_secret, schnorr_sign,
};
use bdk_mweb::hash::blake3_hash;
use bdk_mweb::keys::{MasterKeyScheme, MasterKeys};
use bdk_mweb::lip0006::{sync_mweb_at_tip, VerifyMode};
use bdk_mweb::lip0006_tcp::TcpMwebPeer;
#[allow(deprecated)]
use bdk_mweb::tx_builder::{build_pegin, create_input, create_kernel, create_output};
use bdk_mweb::{output_id, scan_litecoin_tx, AddressBook, MwebCoinDatabase, DEFAULT_GAP_LIMIT};
use bdk_testenv::{relay_policy, try_node_from_env, RelayPolicy, MWEB_PEGIN_MATURITY};
use bitcoin::blockdata::mimblewimble::{
    Output, OutputMessage, OutputMessageStandardFields, Transaction as MwTx, TxBody,
};
use bitcoin::consensus::encode::{deserialize_hex, serialize};
use bitcoin::key::Secp256k1;
use bitcoin::secp256k1::{PublicKey, SecretKey};
use bitcoin::transaction::Version;
use bitcoin::{Amount, Block, Network, NetworkKind, Transaction};
use hex_conservative::{DisplayHex, FromHex};

const SEED_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

/// A compressed encoding (0x02 prefix) whose x has no point on secp256k1.
fn off_curve_key(salt: u8) -> [u8; 33] {
    let mut k = [0u8; 33];
    k[0] = 0x02;
    k[1] = salt;
    for x in 1u8..=255 {
        k[32] = x;
        if PublicKey::from_slice(&k).is_err() {
            return k;
        }
    }
    unreachable!("about half of all x values are off the curve")
}

#[test]
fn offcurve_ko_ke_output_does_not_stall_ldk() {
    let Some(env) = try_node_from_env().expect("harness") else {
        return;
    };
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

    // Fund one owned MWEB coin.
    let mining = env.mine_to_pre_mweb().expect("pre-mweb");
    let pegin_amount = Amount::from_btc(1.0).unwrap().to_sat();
    #[allow(deprecated)]
    let pegin = build_pegin(&keys, 2, pegin_amount, 50_000, NetworkKind::Test, &secp).unwrap();
    let pegin_tx = env
        .broadcast_bdk_pegin(pegin.kernel_id, Amount::from_sat(pegin_amount), pegin.mw_tx)
        .expect("peg-in");
    env.mine_mweb_activation(&mining).unwrap();
    env.mine_blocks(MWEB_PEGIN_MATURITY, &mining).unwrap();
    let mut db = MwebCoinDatabase::new();
    let coin = scan_litecoin_tx(&keys, &book, &pegin_tx, &mut db, &secp)
        .unwrap()
        .pop()
        .expect("owned peg-in coin");

    // Spend it to a poisoned output plus an owned change output.
    let fee = 100_000u64;
    let poison_value = 10_000_000u64;
    let change_value = coin.amount - fee - poison_value;

    let bad_ko = off_curve_key(0xa0);
    let bad_ke = off_curve_key(0xb0);
    let poison_raw_blind = random_secret(&secp);
    let poison_switched = blind_switch(&poison_raw_blind, poison_value, &secp).unwrap();
    let poison_sender = random_secret(&secp);
    let ks = PublicKey::from_secret_key(&secp, &SecretKey::from_slice(&poison_sender).unwrap());
    let commitment = pedersen_commit(poison_value, &poison_switched, &secp).unwrap();
    // Proof and signature commit to the off-curve bytes, exactly as a miner
    // including this output would have them.
    let mut msg_bytes = vec![0x01u8];
    msg_bytes.extend_from_slice(&bad_ke);
    msg_bytes.push(0x42); // view tag
    msg_bytes.extend_from_slice(&[0u8; 8 + 16]); // masked value, masked nonce
    let range_proof = bulletproof_prove(poison_value, &poison_switched, &msg_bytes).unwrap();
    let mut h = blake3::Hasher::new();
    h.update(&commitment);
    h.update(&ks.serialize());
    h.update(&bad_ko);
    h.update(&blake3_hash(&msg_bytes));
    h.update(&blake3_hash(&range_proof));
    let signature = schnorr_sign(&poison_sender, h.finalize().as_bytes()).unwrap();
    let poison = Output {
        commitment,
        sender_public_key: ks,
        receiver_public_key: bad_ko,
        message: OutputMessage {
            features: 0x01,
            standard_fields: Some(OutputMessageStandardFields {
                key_exchange_pubkey: bad_ke,
                view_tag: 0x42,
                masked_value: 0,
                masked_nonce: [0; 16],
            }),
            extra_data: vec![],
        },
        range_proof,
        signature,
    };
    assert_eq!(serialize(&poison.message), msg_bytes);

    let change_addr = keys.address(3, NetworkKind::Test, &secp).unwrap();
    let (change_raw_blind, change_sender, change) =
        create_output(&change_addr, change_value, &secp).unwrap();
    let change_switched = blind_switch(&change_raw_blind, change_value, &secp).unwrap();

    let in_switched = blind_switch(&coin.blind, coin.amount, &secp).unwrap();
    let ephemeral = random_secret(&secp);
    let spend_key = coin.spend_key.expect("spend key");
    let input = create_input(
        &coin.output_id,
        &coin.commitment,
        &ephemeral,
        &spend_key,
        &secp,
    )
    .unwrap();

    let kernel_offset = random_secret(&secp);
    let kernel_blind = blind_sum(
        &[poison_switched, change_switched],
        &[in_switched, kernel_offset],
    )
    .unwrap();
    let stealth_blind = random_secret(&secp);
    let kernel = create_kernel(
        kernel_blind,
        Some(stealth_blind),
        Some(fee as i64),
        None,
        &[],
        &secp,
    )
    .unwrap();
    let stealth_offset = blind_sum(
        &[poison_sender, change_sender, ephemeral],
        &[spend_key, stealth_blind],
    )
    .unwrap();

    let mut outputs = vec![poison, change];
    outputs.sort_by_key(output_id);
    let tx = Transaction {
        version: Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![],
        output: vec![],
        mw_tx: Some(MwTx {
            kernel_offset,
            stealth_offset,
            body: TxBody {
                inputs: vec![input],
                outputs,
                kernels: vec![kernel],
            },
        }),
        is_hog_ex: false,
    };
    let raw_hex = serialize(&tx).to_lower_hex_string();
    assert_eq!(deserialize_hex::<Transaction>(&raw_hex).unwrap(), tx);

    let sent = env
        .rpc
        .call("sendrawtransaction", serde_json::json!([raw_hex, 0]));
    if relay_policy() == RelayPolicy::Strict {
        let err = sent.expect_err("strict relay policy must refuse off-curve Ko/Ke");
        assert!(
            format!("{err:#}").contains("non-standard-mweb-tx"),
            "unexpected reject reason: {err:#}"
        );
        return;
    }
    sent.expect("consensus accepts an output with off-curve Ko/Ke");
    let mined = env.mine_blocks(1, &mining).unwrap()[0];
    let tip_height = env.rpc.get_block_count().unwrap();
    assert_eq!(
        env.rpc
            .call("getblock", serde_json::json!([mined.to_string(), 2]))
            .unwrap()["tx"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "the mined block holds the coinbase and the HogEx"
    );

    // bdk_bitcoind_rpc path: the raw block must decode.
    let block_hex = env
        .rpc
        .call("getblock", serde_json::json!([mined.to_string(), 0]))
        .unwrap();
    let block = deserialize_hex::<Block>(block_hex.as_str().unwrap());
    let block_err = block.as_ref().err().map(|e| e.to_string());

    // LIP-0006 path: a full sync must complete and find the owned change.
    let mut peer = TcpMwebPeer::connect(env.p2p_addr(), Network::Regtest).unwrap();
    let mut synced = MwebCoinDatabase::new();
    let sync = sync_mweb_at_tip(
        &mut peer,
        &keys,
        &book,
        &mut synced,
        &secp,
        mined,
        tip_height,
        VerifyMode::HeaderAndPmmr,
    );
    let sync_err = sync.as_ref().err().map(|e| {
        format!(
            "{e} (banworthy={})",
            bdk_mweb::mweb_sync::is_banworthy_peer_error(e)
        )
    });

    assert!(
        block_err.is_none() && sync_err.is_none(),
        "P-1 reproduced. raw block decode: {block_err:?}; LIP-0006 sync: {sync_err:?}"
    );
    assert_eq!(synced.balance(), change_value);
}
