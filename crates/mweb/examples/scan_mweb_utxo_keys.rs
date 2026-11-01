//! Audit probe: walk a node's entire MWEB UTXO set over LIP-0006 and count
//! outputs whose keys the strict `mweb::Output` decoder would reject.
//!
//! Core consensus never validates an output's receiver key (Ko) or key-exchange
//! key (Ke); since 0.21.5.6 only relay policy does. One such output in the UTXO
//! set makes every `mwebutxos` batch containing it undecodable for bdk_mweb, so
//! this probe parses the wire format itself instead of using `MwebUtxos`.
//!
//! Usage:
//!   LITECOIN_P2P=127.0.0.1:9333 \
//!   LITECOIN_CLI="litecoin-cli -datadir=..." \
//!   cargo run -p bdk_mweb --example scan_mweb_utxo_keys --features lip0006
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bdk_mweb::lip0006::MwebUtxoSource;
use bdk_mweb::lip0006_tcp::TcpMwebPeer;
use bitcoin::consensus::{deserialize, serialize, Decodable};
use bitcoin::hashes::{sha256d, Hash};
use bitcoin::p2p::address::Address;
use bitcoin::p2p::message::{CommandString, NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::p2p::{Magic, ServiceFlags};
use bitcoin::secp256k1::PublicKey;
use bitcoin::{BlockHash, Network, VarInt};

const BATCH: u16 = 4096;
const FROZEN: [&str; 1] = ["2f3a08d9f5ef5f388386c11efe935394b14b524220cff4ec5c81942b82e694f7"];

#[derive(Default)]
struct Tally {
    outputs: u64,
    bad_ks: u64,
    bad_ko: u64,
    bad_ke: u64,
    no_std_fields: u64,
    extra_data: u64,
    frozen_seen: Vec<String>,
    samples: Vec<String>,
}

struct Peer {
    s: TcpStream,
    magic: Magic,
}

impl Peer {
    fn connect(addr: &str) -> Self {
        let s = TcpStream::connect(addr).expect("connect");
        s.set_read_timeout(Some(Duration::from_secs(120))).unwrap();
        let mut p = Peer {
            s,
            magic: Magic::from(Network::Bitcoin),
        };
        let services = ServiceFlags::from(
            u64::from(ServiceFlags::NETWORK) | u64::from(ServiceFlags::WITNESS) | (1u64 << 23),
        );
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        p.send(NetworkMessage::Version(VersionMessage {
            version: 70017,
            services,
            timestamp: now,
            receiver: Address::new(&([0, 0, 0, 0], 0).into(), ServiceFlags::NONE),
            sender: Address::new(&([0, 0, 0, 0], 0).into(), services),
            nonce: 0,
            user_agent: "/bdk_mweb-audit-scan/".into(),
            start_height: 0,
            relay: false,
        }));
        let (mut v, mut va) = (false, false);
        while !(v && va) {
            let (cmd, payload) = p.recv();
            match cmd.as_str() {
                "version" => {
                    p.send(NetworkMessage::Verack);
                    v = true;
                }
                "verack" => va = true,
                "ping" => p.pong(&payload),
                _ => {}
            }
        }
        p
    }

    fn send(&mut self, m: NetworkMessage) {
        self.s
            .write_all(&serialize(&RawNetworkMessage::new(self.magic, m)))
            .unwrap();
    }

    fn send_raw(&mut self, cmd: &str, payload: &[u8]) {
        let mut msg = serialize(&self.magic);
        msg.extend(serialize(&CommandString::try_from(cmd).unwrap()));
        msg.extend((payload.len() as u32).to_le_bytes());
        msg.extend(&sha256d::Hash::hash(payload)[..4]);
        msg.extend(payload);
        self.s.write_all(&msg).unwrap();
    }

    fn pong(&mut self, ping_payload: &[u8]) {
        let nonce = u64::from_le_bytes(ping_payload[..8].try_into().unwrap());
        self.send(NetworkMessage::Pong(nonce));
    }

    fn recv(&mut self) -> (String, Vec<u8>) {
        let mut h = [0u8; 24];
        self.s.read_exact(&mut h).unwrap();
        let cmd = String::from_utf8_lossy(&h[4..16])
            .trim_end_matches('\0')
            .to_string();
        let len = u32::from_le_bytes(h[16..20].try_into().unwrap()) as usize;
        let mut payload = vec![0u8; len];
        self.s.read_exact(&mut payload).unwrap();
        (cmd, payload)
    }

    /// `Ok(payload)` for `mwebutxos`, `Err(())` on `notfound` (tip moved).
    fn get_utxos(&mut self, tip: BlockHash, start: u64, num: u16) -> Result<Vec<u8>, ()> {
        let mut req = serialize(&tip);
        req.extend(serialize(&VarInt(start)));
        req.extend(num.to_le_bytes());
        req.push(0x00); // FULL_UTXO
        self.send_raw("getmwebutxos", &req);
        loop {
            let (cmd, payload) = self.recv();
            match cmd.as_str() {
                "mwebutxos" => return Ok(payload),
                "notfound" => return Err(()),
                "ping" => self.pong(&payload),
                _ => {}
            }
        }
    }
}

struct Cur<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        s
    }
    fn varint(&mut self) -> u64 {
        let mut r = &self.b[self.i..];
        let before = r.len();
        let v = VarInt::consensus_decode(&mut r).unwrap().0;
        self.i += before - r.len();
        v
    }
}

fn valid(k: &[u8]) -> bool {
    PublicKey::from_slice(k).is_ok()
}

/// Parse one FULL_UTXO output leniently, tallying key validity. Mirrors
/// `mweb::Output` field order without validating any field.
fn scan_output(c: &mut Cur, leaf: u64, t: &mut Tally) {
    let commitment = c.take(33);
    let ks = c.take(33);
    let ko = c.take(33);
    let msg_start = c.i;
    let features = c.take(1)[0];
    let mut ke: Option<&[u8]> = None;
    if features & 0x01 != 0 {
        ke = Some(c.take(33));
        c.take(1 + 8 + 16);
    }
    if features & 0x02 != 0 {
        let n = c.varint() as usize;
        c.take(n);
        t.extra_data += 1;
    }
    let msg = &c.b[msg_start..c.i];
    let proof = c.take(675);
    let sig = c.take(64);

    let mut h = blake3::Hasher::new();
    h.update(commitment);
    h.update(ks);
    h.update(ko);
    h.update(blake3::hash(msg).as_bytes());
    h.update(blake3::hash(proof).as_bytes());
    h.update(sig);
    let id = hex(h.finalize().as_bytes());

    t.outputs += 1;
    let mut flags = Vec::new();
    if !valid(ks) {
        t.bad_ks += 1;
        flags.push("Ks");
    }
    if !valid(ko) {
        t.bad_ko += 1;
        flags.push("Ko");
    }
    match ke {
        Some(ke) if !valid(ke) => {
            t.bad_ke += 1;
            flags.push("Ke");
        }
        None => t.no_std_fields += 1,
        _ => {}
    }
    if FROZEN.contains(&id.as_str()) {
        t.frozen_seen.push(format!("leaf {leaf} id {id}"));
    }
    if !flags.is_empty() && t.samples.len() < 50 {
        t.samples
            .push(format!("leaf {leaf} id {id} invalid {}", flags.join("+")));
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn cli(args: &[&str]) -> String {
    let base = std::env::var("LITECOIN_CLI").unwrap_or_else(|_| "litecoin-cli".into());
    let mut parts = base.split_whitespace();
    let out = std::process::Command::new(parts.next().unwrap())
        .args(parts)
        .args(args)
        .output()
        .expect("litecoin-cli");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn main() {
    let addr = std::env::var("LITECOIN_P2P").unwrap_or_else(|_| "127.0.0.1:9333".into());
    let tip: BlockHash = cli(&["getbestblockhash"]).parse().expect("tip");
    let height = cli(&["getblockcount"]);

    // Core's `SegmentFactory::Assemble` serves nothing (and disconnects) unless
    // `start_index` is itself an unspent leaf, so page through the leafset.
    let mut lip = TcpMwebPeer::connect(&addr, Network::Bitcoin).expect("lip0006 peer");
    let leafset = lip.get_leafset(tip).expect("leafset");
    let unspent = leafset.unspent_leaf_indices();
    println!(
        "tip={tip} height={height} unspent_leaves={} peer={addr}",
        unspent.len()
    );

    let mut peer = Peer::connect(&addr);
    let mut tally = Tally::default();
    let mut next = 0usize;
    let mut batches = 0u64;
    while next < unspent.len() {
        let start = unspent[next];
        let Ok(payload) = peer.get_utxos(tip, start, BATCH) else {
            panic!("notfound for {tip} at leaf {start}");
        };
        let mut c = Cur { b: &payload, i: 0 };
        let _block: BlockHash = deserialize(c.take(32)).unwrap();
        let _start_index = c.varint();
        let format = c.take(1)[0];
        assert_eq!(format, 0x00, "peer answered with a non-FULL format");
        let n = c.varint();
        let mut last = None;
        for _ in 0..n {
            let leaf = c.varint();
            scan_output(&mut c, leaf, &mut tally);
            last = Some(leaf);
        }
        batches += 1;
        eprintln!(
            "batch {batches}: {n} utxos, outputs so far {}, last leaf {last:?}",
            tally.outputs
        );
        let Some(last) = last else { break };
        while next < unspent.len() && unspent[next] <= last {
            next += 1;
        }
    }
    assert_eq!(
        tally.outputs as usize,
        unspent.len(),
        "scanned output count must equal the leafset's unspent count"
    );
    println!(
        "outputs={} invalid_Ks={} invalid_Ko={} invalid_Ke={} without_standard_fields={} with_extra_data={}",
        tally.outputs,
        tally.bad_ks,
        tally.bad_ko,
        tally.bad_ke,
        tally.no_std_fields,
        tally.extra_data
    );
    println!("frozen outputs in UTXO set: {:?}", tally.frozen_seen);
    for s in &tally.samples {
        println!("  {s}");
    }
}
