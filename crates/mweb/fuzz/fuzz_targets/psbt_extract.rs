//! PSBT MWEB map extraction (`assemble_mw_tx`), the path every LDK spend and
//! peg-in takes to its wire transaction.
//!
//! Required today: extraction never panics. Required once audit finding P-5 is
//! fixed (enable with `--features audit-red`): whatever extraction accepts must
//! encode to bytes that decode back to the same transaction, i.e. it must
//! canonicalize its input or reject it.

use bdk_mweb_fuzz::Cursor;
use honggfuzz::fuzz;
use litecoin::psbt::mweb::{assemble_mw_tx, MwebInput, MwebKernel, MwebOutput};

/// `Some(bytes)` with probability ~1/2, sized from the cursor.
fn opt_bytes(c: &mut Cursor, max: usize) -> Option<Vec<u8>> {
    (c.u8() & 1 == 1).then(|| c.bytes(max).to_vec())
}

fn opt_arr<const N: usize>(c: &mut Cursor) -> Option<[u8; N]> {
    (c.u8() & 1 == 1).then(|| {
        let mut a = [0u8; N];
        for b in &mut a {
            *b = c.u8();
        }
        a
    })
}

fn kernel(c: &mut Cursor) -> MwebKernel {
    MwebKernel {
        excess_commit: opt_arr::<33>(c),
        stealth_commit: opt_bytes(c, 40),
        fee: (c.u8() & 1 == 1).then(|| c.u64()),
        pegin_amount: (c.u8() & 1 == 1).then(|| c.u64()),
        pegouts: (0..c.u8() % 3).map(|_| c.bytes(40).to_vec()).collect(),
        lock_height: (c.u8() & 1 == 1).then(|| c.u32()),
        features: (c.u8() & 1 == 1).then(|| c.u8()),
        extra_data: opt_bytes(c, 8),
        signature: opt_arr::<64>(c),
        ..Default::default()
    }
}

fn output(c: &mut Cursor) -> MwebOutput {
    MwebOutput {
        commit: opt_arr::<33>(c),
        features: (c.u8() & 1 == 1).then(|| c.u8()),
        sender_pubkey: opt_bytes(c, 33),
        output_pubkey: opt_bytes(c, 33),
        standard_fields: opt_bytes(c, 64),
        range_proof: (c.u8() & 1 == 1).then(|| vec![c.u8(); 675]),
        signature: opt_bytes(c, 64),
        extra_data: opt_bytes(c, 8),
        ..Default::default()
    }
}

fn input(c: &mut Cursor) -> MwebInput {
    MwebInput {
        output_id: opt_arr::<32>(c),
        commit: opt_arr::<33>(c),
        output_pubkey: opt_bytes(c, 33),
        input_pubkey: opt_bytes(c, 33),
        features: (c.u8() & 1 == 1).then(|| c.u8()),
        signature: opt_bytes(c, 64),
        extra_data: opt_bytes(c, 8),
        ..Default::default()
    }
}

fn do_test(data: &[u8]) {
    let mut c = Cursor::new(data);
    let inputs: Vec<_> = (0..c.u8() % 3).map(|_| input(&mut c)).collect();
    let outputs: Vec<_> = (0..c.u8() % 3).map(|_| output(&mut c)).collect();
    let kernels: Vec<_> = (0..1 + c.u8() % 2).map(|_| kernel(&mut c)).collect();
    let Ok(tx) = assemble_mw_tx(c.arr32(), c.arr32(), &inputs, &outputs, &kernels) else {
        return;
    };

    #[cfg(feature = "audit-red")]
    {
        use litecoin::blockdata::mimblewimble::Transaction;
        use litecoin::consensus::encode::{deserialize, serialize};
        let bytes = serialize(&tx);
        let again: Transaction =
            deserialize(&bytes).expect("an extracted MWEB tx must decode from its own encoding");
        assert_eq!(again, tx, "extraction produced a non-canonical MWEB tx");
    }
    #[cfg(not(feature = "audit-red"))]
    let _ = tx;
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
    #[test]
    fn sweep_pseudorandom_corpus() {
        bdk_mweb_fuzz::sweep(5000, super::do_test);
    }
}
