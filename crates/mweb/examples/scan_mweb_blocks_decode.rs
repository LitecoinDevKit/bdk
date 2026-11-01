//! Audit probe: decode every block since MWEB activation with the same strict
//! `Block` decoder `bdk_bitcoind_rpc` uses, and report any block it rejects.
//!
//! Complements `scan_mweb_utxo_keys`: that covers the unspent set served over
//! LIP-0006, this covers spent outputs and kernels in historical block bodies.
//! It also lists kernels whose ID differs between rust-litecoin's bit-driven
//! encoding and Core's presence-driven one (feature bit set, field empty), and
//! kernels Core's relay policy would call non-standard.
//!
//! Usage:
//!   LITECOIN_RPC=127.0.0.1:9332 LITECOIN_COOKIE=/path/to/.cookie \
//!   cargo run --release -p bdk_mweb --example scan_mweb_blocks_decode --features lip0006 \
//!     -- [start_height] [end_height] [threads]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bitcoin::consensus::encode::deserialize_hex;
use bitcoin::Block;
use serde_json::{json, Value};

const MAINNET_MWEB_ACTIVATION: u64 = 2_265_984;
const CHUNK: u64 = 50;

fn b64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in input.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn rpc(addr: &str, auth: &str, body: &Value) -> Value {
    let body = body.to_string();
    let mut s = TcpStream::connect(addr).expect("rpc connect");
    write!(
        s,
        "POST / HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Basic {auth}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut r = BufReader::new(s);
    let mut line = String::new();
    let mut len = None;
    loop {
        line.clear();
        r.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            len = Some(v.trim().parse::<usize>().unwrap());
        }
    }
    let mut buf = Vec::new();
    match len {
        Some(n) => {
            buf.resize(n, 0);
            r.read_exact(&mut buf).unwrap();
        }
        None => {
            r.read_to_end(&mut buf).unwrap();
        }
    }
    serde_json::from_slice(&buf).expect("rpc json")
}

fn batch(addr: &str, auth: &str, method: &str, params: Vec<Value>) -> Vec<Value> {
    let req: Vec<Value> = params
        .into_iter()
        .enumerate()
        .map(|(i, p)| json!({"jsonrpc": "1.0", "id": i, "method": method, "params": p}))
        .collect();
    let mut resp = rpc(addr, auth, &Value::Array(req));
    let arr = resp.as_array_mut().expect("batch array");
    arr.sort_by_key(|v| v["id"].as_u64().unwrap());
    arr.iter_mut()
        .map(|v| {
            assert!(v["error"].is_null(), "{method} error: {}", v["error"]);
            v["result"].take()
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let addr = std::env::var("LITECOIN_RPC").unwrap_or_else(|_| "127.0.0.1:9332".into());
    let cookie =
        std::fs::read_to_string(std::env::var("LITECOIN_COOKIE").expect("LITECOIN_COOKIE"))
            .expect("read cookie");
    let auth = b64(cookie.trim().as_bytes());
    let tip = rpc(
        &addr,
        &auth,
        &json!({"method": "getblockcount", "params": []}),
    )["result"]
        .as_u64()
        .unwrap();
    let start: u64 = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(MAINNET_MWEB_ACTIVATION);
    let end: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(tip);
    let threads: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(6);
    println!("scanning heights {start}..={end} with {threads} threads");

    let next = Arc::new(AtomicU64::new(start));
    let done = Arc::new(AtomicU64::new(0));
    let mweb_blocks = Arc::new(AtomicU64::new(0));
    let failures: Arc<Mutex<Vec<String>>> = Arc::default();
    let odd_kernels: Arc<Mutex<Vec<String>>> = Arc::default();
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let (next, done, mweb_blocks, failures, odd_kernels) = (
                next.clone(),
                done.clone(),
                mweb_blocks.clone(),
                failures.clone(),
                odd_kernels.clone(),
            );
            let (addr, auth) = (addr.clone(), auth.clone());
            std::thread::spawn(move || loop {
                let lo = next.fetch_add(CHUNK, Ordering::Relaxed);
                if lo > end {
                    break;
                }
                let hi = (lo + CHUNK - 1).min(end);
                let heights: Vec<u64> = (lo..=hi).collect();
                let hashes = batch(
                    &addr,
                    &auth,
                    "getblockhash",
                    heights.iter().map(|h| json!([h])).collect(),
                );
                let blocks = batch(
                    &addr,
                    &auth,
                    "getblock",
                    hashes.iter().map(|h| json!([h, 0])).collect(),
                );
                for (h, hex) in heights.iter().zip(blocks) {
                    match deserialize_hex::<Block>(hex.as_str().unwrap()) {
                        Ok(b) => {
                            let Some(mw) = b.mweb_block else { continue };
                            mweb_blocks.fetch_add(1, Ordering::Relaxed);
                            for k in &mw.tx_body.kernels {
                                // Core hashes a presence-based re-serialization, so
                                // these kernels have a different ID under rust-litecoin.
                                let empty_pegout = k.features & 0x04 != 0 && k.pegouts.is_empty();
                                let empty_extra = k.features & 0x20 != 0 && k.extra_data.is_empty();
                                let nonstandard = k.features >= 0x20;
                                let lock_over_i32 =
                                    k.lock_height.is_some_and(|l| l > i32::MAX as u32);
                                if empty_pegout || empty_extra || nonstandard || lock_over_i32 {
                                    let msg = format!(
                                        "height {h}: kernel features={:#04x} pegouts={} extra={}B lock={:?}",
                                        k.features,
                                        k.pegouts.len(),
                                        k.extra_data.len(),
                                        k.lock_height
                                    );
                                    eprintln!("ODD KERNEL {msg}");
                                    odd_kernels.lock().unwrap().push(msg);
                                }
                            }
                        }
                        Err(e) => {
                            let msg = format!("height {h}: {e}");
                            eprintln!("DECODE FAILURE {msg}");
                            failures.lock().unwrap().push(msg);
                        }
                    }
                }
                let d = done.fetch_add(heights.len() as u64, Ordering::Relaxed) + heights.len() as u64;
                if d % 20_000 < CHUNK {
                    eprintln!("progress: {d} blocks decoded");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let failures = failures.lock().unwrap();
    println!(
        "blocks={} with_mweb_block={} decode_failures={}",
        done.load(Ordering::Relaxed),
        mweb_blocks.load(Ordering::Relaxed),
        failures.len()
    );
    for f in failures.iter() {
        println!("  {f}");
    }
    let odd = odd_kernels.lock().unwrap();
    println!("non-canonical or non-standard kernels={}", odd.len());
    for k in odd.iter() {
        println!("  {k}");
    }
}
