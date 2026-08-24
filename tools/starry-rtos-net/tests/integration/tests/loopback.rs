//! Host loopback integration tests.
//!
//! These run the real `starry_client` logic (TCP connect, AXNET/1 framing,
//! heartbeat, RTT, reconnect) against an in-process AXNET/1 echo server, so
//! the protocol and client state machine are validated without needing QEMU
//! or root.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    thread,
    time::Duration,
};

use axnet1::*;
use starry_client::client::{Config, FrameReader, run};

// ---------------------------------------------------------------------------
// Golden frame captured from the C implementation (app/axnet1.c) to prove the
// Rust and C encoders are byte-for-byte wire-compatible.
// ---------------------------------------------------------------------------
const GOLDEN_HEX: &str = concat!(
    "a5010101000000000004cafe1234", // magic,ver,type,flags,plen,seq
    "1122334455667788",             // timestamp_us
    "00000000",                     // error_code
    "54455354",                     // payload "TEST"
    "c5c8d1c8",                     // crc32
);

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn rust_encoder_is_wire_compatible_with_c() {
    let mut frame = vec![0u8; frame_len(4)];
    let n = encode_into(
        &mut frame,
        MSG_CONTROL,
        0,
        0xCAFE_1234,
        0x1122_3344_5566_7788,
        0,
        b"TEST",
    );
    assert_eq!(&frame[..n], hex(GOLDEN_HEX).as_slice());
}

// ---------------------------------------------------------------------------
// In-process AXNET/1 echo server.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum ServerMode {
    /// Serve normally (echo heartbeats, ack controls, push STATUS).
    Normal,
    /// Split every outgoing frame into two writes to exercise stream framing.
    SplitWrites,
    /// Complete the handshake, then close immediately (tests reconnects).
    DropAfterHandshake,
    /// Complete the handshake, then stay silent (tests the timeout path).
    SilentAfterHandshake,
}

fn decode_frame(frame: &[u8]) -> Option<Message<'_>> {
    decode(frame, frame.len()).ok()
}

fn write_all(stream: &mut TcpStream, bytes: &[u8]) {
    let _ = stream.write_all(bytes);
}

fn serve(mut stream: TcpStream, mode: ServerMode) {
    let mut reader = FrameReader::new();
    let mut buf = [0u8; 4096];
    let mut status_clock = std::time::Instant::now();
    let mut first_control_done = false;

    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                reader.push(&buf[..n]);
                while let Some(frame) = reader.next_frame() {
                    let msg = match decode_frame(&frame) {
                        Some(m) => m,
                        None => continue,
                    };
                    let mut reply = Vec::new();
                    match msg.msg_type {
                        MSG_CONTROL => {
                            reply.extend_from_slice(&axnet1::encode(
                                MSG_CONTROL_ACK,
                                0,
                                msg.sequence,
                                now_us(),
                                0,
                                b"OK",
                            ));
                            first_control_done = true;
                        }
                        MSG_HEARTBEAT => {
                            reply.extend_from_slice(&axnet1::encode(
                                MSG_HEARTBEAT,
                                0,
                                msg.sequence,
                                now_us(),
                                0,
                                &[],
                            ));
                        }
                        MSG_STATUS => {
                            reply.extend_from_slice(&axnet1::encode(
                                MSG_STATUS,
                                0,
                                msg.sequence,
                                now_us(),
                                0,
                                b"RUNNING",
                            ));
                        }
                        MSG_ERROR => {}
                        _ => {}
                    }
                    if !reply.is_empty() {
                        match mode {
                            ServerMode::SplitWrites => {
                                let mid = reply.len() / 2;
                                write_all(&mut stream, &reply[..mid]);
                                thread::sleep(Duration::from_millis(5));
                                write_all(&mut stream, &reply[mid..]);
                            }
                            _ => write_all(&mut stream, &reply),
                        }
                    }
                }

                // After the first CONTROL was answered, apply the special
                // per-connection modes so the client sees a completed
                // handshake before the connection misbehaves.
                if first_control_done {
                    match mode {
                        ServerMode::DropAfterHandshake => return,
                        ServerMode::SilentAfterHandshake => {
                            // stay connected but never answer again
                            for _ in 0..200 {
                                thread::sleep(Duration::from_millis(100));
                            }
                            return;
                        }
                        _ => {}
                    }
                }
            }
        }

        // Periodic STATUS push, like the FreeRTOS server.
        if status_clock.elapsed() >= Duration::from_millis(2000) && first_control_done {
            let reply = axnet1::encode(MSG_STATUS, 0, 0, now_us(), 0, b"RUNNING");
            write_all(&mut stream, &reply);
            status_clock = std::time::Instant::now();
        }
    }
}

fn spawn_server(mode: ServerMode) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let conns = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let idx = conns.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let conn_mode = match mode {
                // The first connection gets the special behaviour so the
                // client can observe it and reconnect to a healthy one.
                ServerMode::DropAfterHandshake if idx == 0 => ServerMode::DropAfterHandshake,
                ServerMode::SilentAfterHandshake if idx == 0 => ServerMode::SilentAfterHandshake,
                _ => ServerMode::Normal,
            };
            thread::spawn(move || serve(stream, conn_mode));
        }
    });
    addr
}

fn client_config(addr: SocketAddr) -> Config {
    Config {
        server: "127.0.0.1".parse().unwrap(),
        port: addr.port(),
        requests: 30,
        payload_len: 512,
        heartbeat_ms: 100,
        timeout_ms: 1000,
        duration: Some(Duration::from_secs(30)),
        max_reconnect_ms: 2000,
    }
}

fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------

#[test]
fn normal_session_completes() {
    let addr = spawn_server(ServerMode::Normal);
    let stats = run(&client_config(addr), |_| {});
    assert_eq!(stats.sent, 30, "all requests sent");
    assert_eq!(stats.acked, 30, "all requests acked");
    assert_eq!(stats.errors, 0, "no protocol errors");
    assert_eq!(stats.reconnects, 0, "no reconnect on a healthy server");
    assert_eq!(stats.rtts_us.len(), 30, "one RTT sample per request");
    assert!(stats.throughput_bps() > 0.0, "throughput computed");
}

#[test]
fn split_tcp_writes_still_frame_correctly() {
    let addr = spawn_server(ServerMode::SplitWrites);
    let stats = run(&client_config(addr), |_| {});
    assert_eq!(stats.sent, 30);
    assert_eq!(stats.acked, 30);
    assert_eq!(stats.errors, 0, "split writes must not corrupt framing");
}

#[test]
fn client_reconnects_after_server_drops_session() {
    let addr = spawn_server(ServerMode::DropAfterHandshake);
    let mut cfg = client_config(addr);
    cfg.requests = 10;
    let stats = run(&cfg, |_| {});
    println!("reconnect test stats: {stats:?}");
    assert!(
        stats.reconnects >= 1,
        "client must reconnect after a dropped session"
    );
    assert_eq!(stats.sent, 10, "requests completed after reconnect");
    assert_eq!(stats.acked + stats.lost, 10, "every request resolved");
}

#[test]
fn client_times_out_and_recovers_on_silent_server() {
    let addr = spawn_server(ServerMode::SilentAfterHandshake);
    let mut cfg = client_config(addr);
    cfg.requests = 10;
    cfg.timeout_ms = 300; // short so the test is fast
    let stats = run(&cfg, |_| {});
    println!("timeout test stats: {stats:?}");
    assert!(
        stats.timeouts >= 1,
        "silent server must trigger the timeout path"
    );
    assert!(stats.reconnects >= 1, "client must reconnect after timeout");
    assert_eq!(stats.sent, 10);
    assert_eq!(stats.acked + stats.lost, 10, "every request resolved");
}
