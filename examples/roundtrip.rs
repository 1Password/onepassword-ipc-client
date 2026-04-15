//! Round-trip example: MacOS Server + onepassword-ipc-client crate
//!
//! Run with: `cargo run --example roundtrip`
//!
//! Demonstrates both one-shot (`send_to`) and connection-reuse (`send_with_client`)
//! usage against a local echo server. Tests small messages and large multi-chunk
//! payloads.

use std::pin::pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::Duration;

use futures_util::StreamExt;
use mach_listener::{Client, Server};

static COUNTER: AtomicU32 = AtomicU32::new(0);

// Examples

fn main() {
    // One-shot: small message
    println!("=== One-shot: small message ===");
    let svc = test_service_name();
    let server = spawn_echo_server(&svc, 1);
    thread::sleep(Duration::from_millis(100));

    let response = onepassword_ipc_client::send_to(&svc, b"hello".to_vec()).unwrap();
    assert_eq!(response.data, b"hello");
    println!("OK ({} bytes echoed, responder PID {})", response.data.len(), response.peer_identity.pid());
    server.join().unwrap();

    // One-shot: large multi-chunk message
    println!("\n=== One-shot: large message (1 MB, multi-chunk) ===");
    let svc = test_service_name();
    let server = spawn_echo_server(&svc, 1);
    thread::sleep(Duration::from_millis(100));

    let large: Vec<u8> = (0..1_024 * 1_024).map(|i| (i % 251) as u8).collect();
    let response = onepassword_ipc_client::send_to(&svc, large.clone()).unwrap();
    assert_eq!(response.data, large);
    println!("OK ({} bytes echoed, responder PID {})", response.data.len(), response.peer_identity.pid());
    server.join().unwrap();

    // Connection reuse: multiple messages over one client
    println!("\n=== Connection reuse: 3 messages over one client ===");
    let svc = test_service_name();
    let server = spawn_echo_server(&svc, 3);
    thread::sleep(Duration::from_millis(100));

    let mut client = Client::connect(&svc).unwrap();
    client.set_send_timeout(Some(Duration::from_secs(5)));

    let messages: &[&[u8]] = &[b"first", b"second", b"third"];
    for msg in messages {
        let response = onepassword_ipc_client::send_with_client(&mut client, msg.to_vec()).unwrap();
        assert_eq!(response.data, *msg);
        println!("  {:?} -> OK (responder PID {})", std::str::from_utf8(msg).unwrap(), response.peer_identity.pid());
    }
    server.join().unwrap();

    println!("\nAll round-trips passed!");
}

fn test_service_name() -> String {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("com.ipc-demo.test.{}.{}", std::process::id(), id)
}

// Test Server Infrastructure.
//
// the chunking module is private, this is just used to showcase an example server.
const BYTE_NO_MORE_CHUNKS: u8 = 0x01;
const BYTE_MORE_CHUNKS: u8 = 0x02;
const CHUNK_SIZE: usize = 500_000;
const HEADER_SIZE: usize = 1;
const PAYLOAD_SIZE: usize = CHUNK_SIZE - HEADER_SIZE;

fn build_chunks(payload: &[u8]) -> Vec<Vec<u8>> {
    use itertools::{Itertools, Position};

    if payload.is_empty() {
        return vec![vec![BYTE_NO_MORE_CHUNKS]];
    }

    payload
        .chunks(PAYLOAD_SIZE)
        .with_position()
        .map(|(pos, chunk)| {
            let header = match pos {
                Position::Only | Position::Last => BYTE_NO_MORE_CHUNKS,
                _ => BYTE_MORE_CHUNKS,
            };
            let mut v = Vec::with_capacity(CHUNK_SIZE);
            v.push(header);
            v.extend_from_slice(chunk);
            v
        })
        .collect()
}

fn parse_chunk(message: &[u8]) -> Option<(bool, Vec<u8>)> {
    let last_chunk_marker = message.first()?;
    let payload = Vec::from(message.get(HEADER_SIZE..)?);
    Some((last_chunk_marker == &BYTE_NO_MORE_CHUNKS, payload))
}

/// Spins up a server that reassembles chunked requests and echoes them back.
/// Handles `message_count` sequential request-response exchanges.
fn spawn_echo_server(service_name: &str, message_count: usize) -> thread::JoinHandle<()> {
    let name = service_name.to_owned();
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let mut server = Server::register(&name).unwrap();
            let mut stream = pin!(server.listen());

            for _ in 0..message_count {
                let mut request = Vec::new();
                loop {
                    let mut msg = stream.next().await.unwrap().unwrap();
                    if msg.data.is_empty() {
                        let _ = msg.reply(&[]);
                        continue;
                    }

                    let (is_last, payload) = parse_chunk(&msg.data).unwrap();
                    request.extend_from_slice(&payload);

                    if !is_last {
                        msg.reply(&[]).unwrap();
                        continue;
                    }

                    let chunks = build_chunks(&request);
                    msg.reply(&chunks[0]).unwrap();
                    for chunk in &chunks[1..] {
                        let mut next = stream.next().await.unwrap().unwrap();
                        next.reply(chunk).unwrap();
                    }
                    break;
                }
            }
        });
    })
}
