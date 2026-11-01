//! Reorgs (`invalidateblock`) across a BDK peg-in and peg-out.
//!
//! A wallet that keeps one `SyncState` and `MwebCoinDatabase` across passes
//! must track the chain through two reorgs: one that disconnects the peg-out,
//! and one that disconnects the peg-in under it, then re-mines both. After
//! every pass its unspent set must equal what a from-scratch sync sees at the
//! same tip, and value must be conserved: MWEB unspent plus the confirmed
//! peg-out and its fee equals what the peg-in delivered.
//!
//! Needs `LITECOIND_EXE` (skips when unset).

#![cfg(feature = "lip0006")]

use std::collections::BTreeMap;

use bdk_mweb::keys::{MasterKeyScheme, MasterKeys};
use bdk_mweb::lip0006::VerifyMode;
use bdk_mweb::lip0006_tcp::TcpMwebPeer;
use bdk_mweb::mweb_sync::{BlockHeaderProvider, MwebSyncer, ReadyNotifier, SyncState};
#[allow(deprecated)]
use bdk_mweb::tx_builder::{build_pegin, MwebTxBuilder, CHANGE_ADDRESS_INDEX};
use bdk_mweb::{AddressBook, Error, MwebCoinDatabase, DEFAULT_GAP_LIMIT};
use bdk_testenv::{try_node_from_env, LitecoinNodeEnv, FIRST_MWEB_HEIGHT, MWEB_PEGIN_MATURITY};
use bitcoin::key::Secp256k1;
use bitcoin::secp256k1::All;
use bitcoin::{Address, Amount, BlockHash, Network, NetworkKind};
use hex_conservative::FromHex;

const SEED_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

/// The node's own chain, standing in for the wallet's trusted header source.
///
/// Heights below MWEB activation are reported as unknown so header sampling
/// skips them rather than asking the peer for MWEB headers that do not exist.
struct NodeHeaders<'a>(&'a LitecoinNodeEnv);

impl BlockHeaderProvider for NodeHeaders<'_> {
    fn tip(&self) -> Result<(BlockHash, u32), Error> {
        let rpc = &self.0.rpc;
        let height = rpc
            .get_block_count()
            .map_err(|e| Error::Crypto(e.to_string()))?;
        let hash = rpc
            .get_block_hash(height)
            .map_err(|e| Error::Crypto(e.to_string()))?;
        Ok((hash, height))
    }

    fn block_hash_at(&self, height: u32) -> Result<BlockHash, Error> {
        if height < FIRST_MWEB_HEIGHT {
            return Err(Error::Crypto(format!("height {height} is pre-MWEB")));
        }
        self.0
            .rpc
            .get_block_hash(height)
            .map_err(|e| Error::Crypto(e.to_string()))
    }
}

struct Wallet {
    keys: MasterKeys,
    book: AddressBook,
    secp: Secp256k1<All>,
    state: SyncState,
    db: MwebCoinDatabase,
}

impl Wallet {
    fn new() -> Self {
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
        Self {
            keys,
            book,
            secp,
            state: SyncState::default(),
            db: MwebCoinDatabase::new(),
        }
    }

    fn sync(&mut self, env: &LitecoinNodeEnv) {
        let headers = NodeHeaders(env);
        let (_, tip_height) = headers.tip().unwrap();
        let syncer = MwebSyncer {
            verify: VerifyMode::Anchored,
            fine_window: 64,
            ..MwebSyncer::default()
        };
        let mut peer = TcpMwebPeer::connect(env.p2p_addr(), Network::Regtest).unwrap();
        syncer
            .run_once(
                &headers,
                &mut ReadyNotifier { tip_height },
                &mut peer,
                &mut self.state,
                &self.keys,
                &self.book,
                &mut self.db,
                &self.secp,
                None,
            )
            .expect("anchored sync against the regtest node");
    }

    fn unspent(&self) -> BTreeMap<[u8; 32], u64> {
        self.db.unspent().map(|c| (c.output_id, c.amount)).collect()
    }
}

/// Sync `wallet` incrementally, then check it against a from-scratch sync.
#[track_caller]
fn sync_and_check(env: &LitecoinNodeEnv, wallet: &mut Wallet, step: &str) {
    wallet.sync(env);
    let mut fresh = Wallet::new();
    fresh.sync(env);
    assert_eq!(
        wallet.unspent(),
        fresh.unspent(),
        "{step}: incremental unspent set differs from a fresh sync"
    );
}

fn received(env: &LitecoinNodeEnv, addr: &Address) -> Amount {
    let v = env
        .rpc
        .call(
            "getreceivedbyaddress",
            serde_json::json!([addr.to_string(), 1]),
        )
        .unwrap();
    Amount::from_btc(v.as_f64().unwrap()).unwrap()
}

#[test]
fn reorg_across_pegin_and_pegout_conserves_value() {
    let Some(env) = try_node_from_env().expect("harness") else {
        return;
    };
    let mut wallet = Wallet::new();

    // Activate MWEB with a Core-wallet peg-in the BDK wallet does not own, so
    // the BDK peg-in below lands in its own post-activation block that can be
    // disconnected without deactivating MWEB.
    let mining = env.mine_to_pre_mweb().expect("pre-mweb");
    let core_mweb = env.rpc.get_new_mweb_address().expect("core mweb address");
    env.finalize_mweb_pegin(&core_mweb, Amount::from_btc(1.0).unwrap())
        .expect("activation peg-in");
    env.mine_mweb_activation(&mining).expect("activate");

    let pegin_amount = Amount::from_btc(1.0).unwrap().to_sat();
    let mweb_fee = 50_000u64;
    let delivered = pegin_amount - mweb_fee;
    #[allow(deprecated)]
    let pegin = build_pegin(
        &wallet.keys,
        2,
        pegin_amount,
        mweb_fee,
        NetworkKind::Test,
        &wallet.secp,
    )
    .unwrap();
    env.broadcast_bdk_pegin(pegin.kernel_id, Amount::from_sat(pegin_amount), pegin.mw_tx)
        .expect("BDK peg-in");
    let pegin_block = env.mine_blocks(1, &mining).unwrap()[0];
    env.mine_blocks(MWEB_PEGIN_MATURITY, &mining).unwrap();

    sync_and_check(&env, &mut wallet, "after peg-in");
    let tip = env.rpc.get_block_count().unwrap();
    assert_eq!(wallet.db.balance(), delivered);
    assert_eq!(
        wallet.db.balance_at(tip).confirmed,
        delivered,
        "peg-in is mature"
    );
    let coin = wallet.db.unspent_vec().pop().expect("synced peg-in coin");
    assert!(coin.spend_key.is_some());

    // Peg-out part of it, with change back to the wallet.
    let pegout_addr = env.rpc.get_new_address().unwrap();
    let pegout_amt = Amount::from_btc(0.3).unwrap();
    let pegout_fee = 50_000u64;
    let change = delivered - pegout_amt.to_sat() - pegout_fee;
    let finished = MwebTxBuilder::new()
        .add_input(coin.clone())
        .add_pegout(pegout_addr.script_pubkey(), pegout_amt.to_sat())
        .fee(pegout_fee)
        .finish(
            &wallet.keys,
            CHANGE_ADDRESS_INDEX,
            NetworkKind::Test,
            &wallet.secp,
        )
        .expect("peg-out");
    env.rpc
        .send_raw_transaction(&finished.tx)
        .expect("send peg-out");
    let pegout_block = env.mine_blocks(1, &mining).unwrap()[0];

    sync_and_check(&env, &mut wallet, "after peg-out");
    assert_eq!(wallet.db.balance(), change);
    assert_eq!(received(&env, &pegout_addr), pegout_amt);
    assert_eq!(
        wallet.db.balance() + pegout_amt.to_sat() + pegout_fee,
        delivered
    );

    // Reorg 1: disconnect the peg-out. The peg-in coin is unspent on chain
    // again and the change output no longer exists.
    env.rpc.invalidate_block(&pegout_block).unwrap();
    sync_and_check(&env, &mut wallet, "after disconnecting the peg-out");
    assert_eq!(received(&env, &pegout_addr), Amount::ZERO);
    assert_eq!(wallet.db.balance(), delivered);
    assert!(
        wallet.db.get(&coin.output_id).is_some(),
        "peg-in coin is unspent again"
    );

    // Reorg 2: disconnect the peg-in block (and everything after it).
    env.rpc.invalidate_block(&pegin_block).unwrap();
    sync_and_check(&env, &mut wallet, "after disconnecting the peg-in");
    assert_eq!(received(&env, &pegout_addr), Amount::ZERO);
    assert_eq!(wallet.db.balance(), 0, "nothing of ours is on chain");

    // Re-mine. Core returned both disconnected transactions to its mempool,
    // the peg-out chained on the peg-in, so the next block confirms both.
    env.mine_blocks(1 + MWEB_PEGIN_MATURITY, &mining).unwrap();
    sync_and_check(&env, &mut wallet, "after re-mining");
    // The peg-out is MWEB-only, so the node's record of it is the HogEx
    // payment to the watched address.
    assert_eq!(received(&env, &pegout_addr), pegout_amt);
    assert_eq!(wallet.db.balance(), change);
    assert!(
        wallet.db.get(&coin.output_id).is_none(),
        "peg-in coin spent again"
    );
    assert_eq!(
        wallet.db.balance() + pegout_amt.to_sat() + pegout_fee,
        delivered
    );
}
