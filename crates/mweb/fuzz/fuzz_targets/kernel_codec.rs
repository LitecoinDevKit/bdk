//! Kernel codec differential against Litecoin Core's `Kernel::Serialize`.
//!
//! Core reads a kernel field whenever its feature bit is set but writes pegouts
//! and extra data only when non-empty, and hashes its own re-serialization for
//! the kernel ID. rust-litecoin is bit-driven both ways. The two must agree on
//! every kernel whose set bits all carry non-empty fields; the others (the
//! empty-pegout and empty-extra-data encodings soft-forked out in 0.21.5.6 and
//! 0.21.5.7) are where `kernel_id` diverges from Core (audit finding P-9).

use bdk_mweb::hash::blake3_hash;
use bdk_mweb::kernel_id;
use honggfuzz::fuzz;
use litecoin::blockdata::mimblewimble::Kernel;
use litecoin::consensus::encode::{deserialize, serialize};
use litecoin::VarInt;

const PEGOUT: u8 = 0x04;
const EXTRA: u8 = 0x20;

fn core_varint(mut n: u64, out: &mut Vec<u8>) {
    let mut tmp = Vec::new();
    loop {
        tmp.push((n & 0x7f) as u8 | if tmp.is_empty() { 0 } else { 0x80 });
        if n <= 0x7f {
            break;
        }
        n = (n >> 7) - 1;
    }
    out.extend(tmp.iter().rev());
}

/// Core `Kernel::Serialize` (presence-driven).
fn core_kernel_bytes(k: &Kernel) -> Vec<u8> {
    let mut out = vec![k.features];
    if let Some(fee) = k.fee {
        core_varint(fee as u64, &mut out);
    }
    if let Some(pegin) = k.pegin {
        core_varint(pegin as u64, &mut out);
    }
    if !k.pegouts.is_empty() {
        out.extend(serialize(&VarInt(k.pegouts.len() as u64)));
        for p in &k.pegouts {
            core_varint(p.amount as u64, &mut out);
            out.extend(serialize(&p.script_pub_key));
        }
    }
    if let Some(lock) = k.lock_height {
        core_varint(lock as u64, &mut out);
    }
    if let Some(se) = k.stealth_excess {
        out.extend(se.serialize());
    }
    if !k.extra_data.is_empty() {
        out.extend(serialize(&k.extra_data));
    }
    out.extend(k.excess);
    out.extend(k.signature);
    out
}

fn do_test(data: &[u8]) {
    let Ok(k) = deserialize::<Kernel>(data) else {
        return;
    };
    // Decoded values are never negative, so the encoder cannot hit its
    // one-byte negative path here.
    assert!(k.fee.map_or(true, |v| v >= 0));
    assert!(k.pegin.map_or(true, |v| v >= 0));
    assert!(k
        .pegouts
        .iter()
        .all(|p| p.amount >= 0 && !p.script_pub_key.is_empty()));

    let ours = serialize(&k);
    let again: Kernel = deserialize(&ours).expect("re-encoded kernel must decode");
    assert_eq!(again, k, "kernel round trip changed value");

    let canonical = !(k.features & PEGOUT != 0 && k.pegouts.is_empty())
        && !(k.features & EXTRA != 0 && k.extra_data.is_empty());
    if canonical {
        assert_eq!(
            ours,
            core_kernel_bytes(&k),
            "canonical kernel encodes unlike Core"
        );
        assert_eq!(kernel_id(&k), blake3_hash(&core_kernel_bytes(&k)));
    } else {
        assert_ne!(ours, core_kernel_bytes(&k));
    }
}

fn main() {
    loop {
        fuzz!(|data| {
            do_test(data);
        });
    }
}

#[cfg(test)]
mod tests {
    use litecoin::blockdata::mimblewimble::{Kernel, PegOutCoin};
    use litecoin::consensus::encode::serialize;
    use litecoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
    use litecoin::ScriptBuf;

    #[test]
    fn sweep_pseudorandom_corpus() {
        bdk_mweb_fuzz::sweep(5000, super::do_test);
    }

    #[test]
    fn sweep_every_feature_combination() {
        let pk = PublicKey::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[3u8; 32]).unwrap(),
        );
        for features in 0u8..=0xff {
            for empty in [false, true] {
                let k = Kernel {
                    features,
                    fee: (features & 0x01 != 0).then_some(1_000),
                    pegin: (features & 0x02 != 0).then_some(2_000),
                    pegouts: if features & 0x04 != 0 && !empty {
                        vec![PegOutCoin {
                            amount: 3_000,
                            script_pub_key: ScriptBuf::from_bytes(vec![0x51]),
                        }]
                    } else {
                        vec![]
                    },
                    lock_height: (features & 0x08 != 0).then_some(4_000),
                    stealth_excess: (features & 0x10 != 0).then_some(pk),
                    extra_data: if features & 0x20 != 0 && !empty {
                        vec![1, 2]
                    } else {
                        vec![]
                    },
                    excess: pk.serialize(),
                    signature: [9; 64],
                };
                super::do_test(&serialize(&k));
            }
        }
    }
}
