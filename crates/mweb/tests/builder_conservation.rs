//! Property test: every spend `MwebTxBuilder` authors satisfies both MWEB
//! balance equations Core checks in consensus.
//!
//! Commitments: `Σ C_out − Σ C_in + (fee + Σ pegout − pegin)·H = Σ excess + offset·G`.
//! Stealth (`StealthSumValidator`): `Σ Ks + Σ Ki = Σ stealth_excess + Σ Ko_in + stealth_offset·G`.

use bdk_mweb::crypto::{commitment_to_pubkey, pedersen_commit, switch_commit};
use bdk_mweb::keys::{MasterKeyScheme, MasterKeys};
use bdk_mweb::{MwebCoin, MwebTxBuilder, CHANGE_ADDRESS_INDEX};
use bitcoin::key::Secp256k1;
use bitcoin::secp256k1::{PublicKey, SecretKey};
use bitcoin::{Network, NetworkKind, ScriptBuf};

/// Deterministic xorshift so failures reproduce.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn secret(&mut self) -> [u8; 32] {
        let mut s = [0u8; 32];
        for chunk in s.chunks_mut(8) {
            chunk.copy_from_slice(&self.next().to_le_bytes());
        }
        s[0] &= 0x7f; // keep below the group order
        s
    }
}

fn sum(keys: &[PublicKey]) -> PublicKey {
    let refs: Vec<&PublicKey> = keys.iter().collect();
    PublicKey::combine_keys(&refs).unwrap()
}

#[test]
fn builder_spends_balance_commitments_and_stealth_sums() {
    let secp = Secp256k1::new();
    let keys = MasterKeys::from_seed(
        &[5u8; 32],
        Network::Regtest,
        MasterKeyScheme::LitecoinCore,
        &secp,
    )
    .unwrap();
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    // A fixed blind for the value-only terms: C(v, r) − C(0, r) = v·H.
    let r = rng.secret();
    let value_h = |v: u64| -> Option<PublicKey> {
        (v > 0).then(|| {
            let with_v = commitment_to_pubkey(&pedersen_commit(v, &r, &secp).unwrap()).unwrap();
            let zero = commitment_to_pubkey(&pedersen_commit(0, &r, &secp).unwrap()).unwrap();
            with_v.combine(&zero.negate(&secp)).unwrap()
        })
    };

    for case in 0..24 {
        let coins: Vec<MwebCoin> = (0..1 + rng.below(3))
            .map(|i| {
                let amount = 1_000_000 + rng.below(50_000_000);
                let blind = rng.secret();
                MwebCoin {
                    output_id: [case as u8 ^ i as u8; 32].map(|b| b ^ rng.below(256) as u8),
                    commitment: switch_commit(&blind, amount, &secp).unwrap(),
                    amount,
                    address_index: 2,
                    blind,
                    shared_secret: rng.secret(),
                    spend_key: Some(rng.secret()),
                    block_height: Some(500),
                    is_pegin: false,
                    leaf_index: None,
                }
            })
            .collect();
        let total: u64 = coins.iter().map(|c| c.amount).sum();
        let fee = 1_000 + rng.below(100_000);
        let mut budget = total - fee;
        let mut b = MwebTxBuilder::new().fee(fee);
        for c in &coins {
            b = b.add_input(c.clone());
        }
        let mut pegout_total = 0;
        for i in 0..rng.below(3) {
            let amt = rng.below(budget / 4 + 1);
            budget -= amt;
            let addr = keys
                .address(20 + i as u32, NetworkKind::Test, &secp)
                .unwrap();
            b = b.add_recipient(addr, amt);
        }
        for _ in 0..rng.below(2) {
            let amt = 1 + rng.below(budget / 4 + 1);
            budget -= amt;
            pegout_total += amt;
            b = b.add_pegout(ScriptBuf::new_op_return([1u8]), amt);
        }
        let finished = b
            .finish(&keys, CHANGE_ADDRESS_INDEX, NetworkKind::Test, &secp)
            .unwrap();
        let mw = finished.tx.mw_tx.as_ref().unwrap();
        assert_eq!(mw.body.inputs.len(), coins.len());

        // Commitment balance.
        let mut lhs: Vec<PublicKey> = mw
            .body
            .outputs
            .iter()
            .map(|o| commitment_to_pubkey(&o.commitment).unwrap())
            .collect();
        lhs.extend(
            mw.body
                .inputs
                .iter()
                .map(|i| commitment_to_pubkey(&i.commitment).unwrap().negate(&secp)),
        );
        lhs.extend(value_h(fee + pegout_total));
        let mut rhs: Vec<PublicKey> = mw
            .body
            .kernels
            .iter()
            .map(|k| commitment_to_pubkey(&k.excess).unwrap())
            .collect();
        rhs.push(PublicKey::from_secret_key(
            &secp,
            &SecretKey::from_slice(&mw.kernel_offset).unwrap(),
        ));
        assert_eq!(
            sum(&lhs),
            sum(&rhs),
            "case {case}: commitments do not balance"
        );

        // Stealth balance.
        let mut lhs: Vec<PublicKey> = mw
            .body
            .outputs
            .iter()
            .map(|o| o.sender_public_key)
            .collect();
        lhs.extend(mw.body.inputs.iter().filter_map(|i| i.input_public_key));
        let mut rhs: Vec<PublicKey> = mw
            .body
            .kernels
            .iter()
            .filter_map(|k| k.stealth_excess)
            .collect();
        rhs.extend(mw.body.inputs.iter().map(|i| i.output_public_key));
        rhs.push(PublicKey::from_secret_key(
            &secp,
            &SecretKey::from_slice(&mw.stealth_offset).unwrap(),
        ));
        assert_eq!(
            sum(&lhs),
            sum(&rhs),
            "case {case}: stealth sums do not balance"
        );
    }
}
