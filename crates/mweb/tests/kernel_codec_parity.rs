//! Kernel and PSBT-extract codec checks against Litecoin Core 0.21.5.8.
//!
//! Core's `Kernel::Unserialize` reads a field whenever its feature bit is set,
//! but `Kernel::Serialize` writes pegouts and extra data only when they are
//! non-empty, and the kernel ID hashes Core's own re-serialization. rust-litecoin
//! is bit-driven in both directions, so the two agree exactly when every set bit
//! has a non-empty field. `core_kernel_bytes` models Core's writer.
//!
//! Tests marked `#[ignore]` assert the behaviour we want and fail today; each
//! names the audit finding it pins.

use bdk_mweb::hash::blake3_hash;
use bdk_mweb::kernel_id;
use bitcoin::blockdata::mimblewimble::{Kernel, OutputMessage, PegOutCoin};
use bitcoin::consensus::{deserialize, serialize};
use bitcoin::psbt::mweb::{assemble_mw_tx, pegout_psbt_value, MwebKernel, MwebOutput};
use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
use bitcoin::{ScriptBuf, VarInt};

const FEE: u8 = 0x01;
const PEGIN: u8 = 0x02;
const PEGOUT: u8 = 0x04;
const HEIGHT_LOCK: u8 = 0x08;
const STEALTH: u8 = 0x10;
const EXTRA: u8 = 0x20;

/// Core `WriteVarInt<VarIntMode::NONNEGATIVE_SIGNED>`.
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

fn pubkey(byte: u8) -> PublicKey {
    PublicKey::from_secret_key(
        &Secp256k1::new(),
        &SecretKey::from_slice(&[byte; 32]).unwrap(),
    )
}

/// A kernel whose fields are populated exactly for the bits in `features`.
fn canonical_kernel(features: u8) -> Kernel {
    Kernel {
        features,
        fee: (features & FEE != 0).then_some(1_000_000),
        pegin: (features & PEGIN != 0).then_some(250_000_000),
        pegouts: if features & PEGOUT != 0 {
            vec![PegOutCoin {
                amount: 30_000_000,
                script_pub_key: ScriptBuf::from_bytes(
                    vec![0x00, 0x14].into_iter().chain([7; 20]).collect(),
                ),
            }]
        } else {
            vec![]
        },
        lock_height: (features & HEIGHT_LOCK != 0).then_some(3_210_000),
        stealth_excess: (features & STEALTH != 0).then(|| pubkey(3)),
        extra_data: if features & EXTRA != 0 {
            vec![0xde, 0xad]
        } else {
            vec![]
        },
        excess: pubkey(1).serialize(),
        signature: [0x55; 64],
    }
}

#[test]
fn canonical_kernels_encode_like_core_for_every_feature_combination() {
    for features in 0u8..0x40 {
        let k = canonical_kernel(features);
        let ours = serialize(&k);
        assert_eq!(ours, core_kernel_bytes(&k), "features {features:#04x}");
        assert_eq!(kernel_id(&k), blake3_hash(&ours));
        assert_eq!(
            deserialize::<Kernel>(&ours).unwrap(),
            k,
            "round trip {features:#04x}"
        );
    }
}

/// P-9: Core computes the ID over its own presence-driven re-serialization, so
/// for the two soft-forked encodings the kernel ID LDK computes is not Core's.
/// Both were consensus-valid on mainnet before heights 3,154,440 and 3,172,640.
#[test]
#[ignore = "P-9 red test: kernel_id hashes the wire bytes, Core hashes its re-serialization"]
fn noncanonical_kernel_id_matches_core() {
    for features in [FEE | PEGOUT | STEALTH, FEE | STEALTH | EXTRA] {
        let mut k = canonical_kernel(features);
        k.pegouts.clear();
        k.extra_data.clear();
        let wire = serialize(&k);
        let parsed: Kernel = deserialize(&wire).unwrap();
        assert_eq!(
            kernel_id(&parsed),
            blake3_hash(&core_kernel_bytes(&parsed)),
            "features {features:#04x}"
        );
    }
}

fn kernel_map(features: Option<u8>, fee: Option<u64>) -> MwebKernel {
    MwebKernel {
        excess_commit: Some(pubkey(1).serialize()),
        stealth_commit: Some(pubkey(3).serialize().to_vec()),
        fee,
        features,
        signature: Some([0x55; 64]),
        ..Default::default()
    }
}

fn extract_kernel(map: MwebKernel) -> Result<Kernel, bitcoin::psbt::Error> {
    assemble_mw_tx([0; 32], [0; 32], &[], &[], &[map]).map(|tx| tx.body.kernels[0].clone())
}

/// P-5: a fee above `i64::MAX` wraps negative in `wire_kernel_from_map`, and
/// `write_amount` encodes any negative value as one byte.
#[test]
#[ignore = "P-5 red test: PSBT extract casts the kernel fee with `as i64`"]
fn extract_rejects_fee_above_i64_max() {
    let fee = i64::MAX as u64 + 2;
    let res = extract_kernel(kernel_map(Some(FEE | STEALTH), Some(fee)));
    assert!(
        res.is_err(),
        "extracted {:?}",
        res.map(|k| (k.fee, serialize(&k)[..3].to_vec()))
    );
}

/// P-5: with no features key the extractor uses 0, so the fee field is kept in
/// the struct but dropped from the wire bytes the kernel signature is checked
/// against.
#[test]
#[ignore = "P-5 red test: missing kernel features default to 0 instead of being derived or rejected"]
fn extract_does_not_drop_fields_when_features_missing() {
    let k = extract_kernel(kernel_map(None, Some(1_000_000))).unwrap();
    let reparsed: Kernel = deserialize(&serialize(&k)).unwrap();
    assert_eq!(
        reparsed, k,
        "the extracted kernel does not survive its own encoding"
    );
}

/// P-5: the extractor will emit the empty-pegout encoding that Core rejects in
/// consensus from mainnet height 3,154,440 (and from genesis on regtest).
#[test]
#[ignore = "P-5 red test: extract emits soft-fork-invalid kernel encodings"]
fn extract_rejects_pegout_bit_without_pegouts() {
    let res = extract_kernel(kernel_map(Some(FEE | PEGOUT | STEALTH), Some(1_000)));
    assert!(
        res.is_err(),
        "extracted a pegout-bit kernel with no pegouts"
    );
}

/// P-5: `pegout_coin_from_psbt_value` accepts an empty script, which the wire
/// decoder (and Core's `PegOutCoin` unserialize) reject.
#[test]
#[ignore = "P-5 red test: PSBT peg-out values with an empty script are accepted"]
fn extract_rejects_empty_pegout_script() {
    let empty = pegout_psbt_value(&PegOutCoin {
        amount: 1_000,
        script_pub_key: ScriptBuf::new(),
    });
    let mut map = kernel_map(Some(FEE | PEGOUT | STEALTH), Some(1_000));
    map.pegouts = vec![empty];
    assert!(extract_kernel(map).is_err());
}

fn output_map(features: Option<u8>, standard_fields_len: usize) -> MwebOutput {
    let mut fields = pubkey(4).serialize().to_vec();
    fields.resize(standard_fields_len, 0);
    MwebOutput {
        commit: Some(pubkey(5).serialize()),
        features,
        sender_pubkey: Some(pubkey(6).serialize().to_vec()),
        output_pubkey: Some(pubkey(7).serialize().to_vec()),
        standard_fields: Some(fields),
        range_proof: Some(vec![0; 675]),
        signature: Some(vec![0; 64]),
        ..Default::default()
    }
}

/// P-5: a truncated standard-fields blob is silently dropped while the
/// standard-fields feature bit stays set, so the encoder writes a message the
/// decoder (and Core) will mis-parse.
#[test]
#[ignore = "P-5 red test: short standard_fields are dropped instead of rejected"]
fn extract_rejects_truncated_standard_fields() {
    let res = assemble_mw_tx([0; 32], [0; 32], &[], &[output_map(Some(0x01), 57)], &[]);
    assert!(
        res.is_err(),
        "extracted an output whose message lost its standard fields"
    );
}

/// The rust-litecoin `OutputMessage` encoder writes standard fields when they
/// are present but the decoder reads them only when the feature bit is set, so
/// a message built with `features = 0` and `Some(standard_fields)` does not
/// round-trip. The extractor produces exactly that when the map has no
/// features key.
#[test]
#[ignore = "P-5 red test: OutputMessage encode is presence-driven, decode is bit-driven"]
fn output_message_round_trips_when_features_missing() {
    let tx = assemble_mw_tx([0; 32], [0; 32], &[], &[output_map(None, 58)], &[]).unwrap();
    let msg = &tx.body.outputs[0].message;
    let reparsed: Result<OutputMessage, _> = deserialize(&serialize(msg));
    assert_eq!(reparsed.ok().as_ref(), Some(msg));
}
