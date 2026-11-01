//! Coin-database parity with Litecoin Core 0.21.5.8's wallet.
//!
//! All tests here are red today; each names the audit finding it pins.

#![cfg(feature = "persist")]

use bdk_mweb::crypto::switch_commit;
use bdk_mweb::{ChangeSet, MwebCoin, MwebCoinDatabase, MWEB_PEGIN_MATURITY};
use bitcoin::secp256k1::Secp256k1;
use hex_conservative::FromHex;

/// Core `GetFrozenMWEBOutputIDs()` (`chainparams.cpp`), mainnet and regtest.
/// Present in the mainnet UTXO set at leaf 308,526 (height 3,205,231).
const FROZEN_OUTPUT_ID: &str = "2f3a08d9f5ef5f388386c11efe935394b14b524220cff4ec5c81942b82e694f7";

fn coin(output_id: [u8; 32], amount: u64, height: u32, is_pegin: bool) -> MwebCoin {
    MwebCoin {
        output_id,
        commitment: [8; 33],
        amount,
        address_index: 2,
        blind: [2; 32],
        shared_secret: [3; 32],
        spend_key: Some([4; 32]),
        block_height: Some(height),
        is_pegin,
        leaf_index: None,
    }
}

/// P-4: Core rejects any spend of a frozen output in consensus and its wallet
/// drops them from balance and coin selection (`wallet.cpp` `IsFrozenMWEBOutput`).
#[test]
#[ignore = "P-4 red test: frozen MWEB output IDs are not excluded"]
fn frozen_output_is_neither_balance_nor_spendable() {
    let frozen: [u8; 32] = <[u8; 32]>::from_hex(FROZEN_OUTPUT_ID).unwrap();
    let mut db = MwebCoinDatabase::new();
    db.insert(coin(frozen, 1_000_000, 100, false));
    db.insert(coin([1; 32], 5_000, 100, false));
    assert_eq!(db.balance_at(200).confirmed, 5_000);
    let spendable = db.unspent_spendable(200, MWEB_PEGIN_MATURITY);
    assert!(spendable.iter().all(|c| c.output_id != frozen));
}

/// P-11: a peg-in confirmed once is counted as `confirmed`, and
/// `CombinedBalance::trusted_spendable` adds that bucket, while coin selection
/// (`unspent_spendable`) correctly withholds it for `MWEB_PEGIN_MATURITY` blocks.
#[test]
#[ignore = "P-11 red test: balance buckets ignore peg-in maturity"]
fn immature_pegin_is_not_in_confirmed_bucket() {
    let mut db = MwebCoinDatabase::new();
    db.insert(coin([1; 32], 1_000_000, 100, true));
    let tip = 100; // one confirmation
    assert!(db.unspent_spendable(tip, MWEB_PEGIN_MATURITY).is_empty());
    assert_eq!(db.balance_at(tip).confirmed, 0);
}

/// P-12: a persisted coin is trusted on load. Its commitment must equal
/// `switch_commit(blind, amount)`, or the stored amount (or blind) is wrong and
/// the coin can never be spent.
#[test]
#[ignore = "P-12 red test: coins are not revalidated on load"]
fn load_rejects_coin_whose_commitment_does_not_open() {
    let secp = Secp256k1::new();
    let mut good = coin([1; 32], 1_000_000, 100, false);
    good.commitment = switch_commit(&good.blind, good.amount, &secp).unwrap();
    let mut tampered = good.clone();
    tampered.output_id = [2; 32];
    tampered.amount = 9_000_000;

    let mut cs = ChangeSet::default();
    cs.coins.insert(good.output_id, good);
    cs.coins.insert(tampered.output_id, tampered);
    let db = MwebCoinDatabase::from_changeset(cs);
    assert_eq!(
        db.balance(),
        1_000_000,
        "a coin whose commitment does not open was loaded"
    );
}
