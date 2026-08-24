//! `rtos-tester` -- host-side validator for the FreeRTOS AXNET/1 server.
//!
//! Intended to run against the RTOS guest in QEMU. With a user-mode netdev
//! the port is forwarded, e.g.:
//!
//!   -netdev user,id=net0,hostfwd=tcp::5000-:5000
//!
//! and the tester connects to 127.0.0.1:5000. It checks the full server
//! behaviour: handshake, CONTROL/ACK, HEARTBEAT echo, periodic STATUS push
//! and the ERROR reply to a corrupt frame.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::{Duration, Instant},
};

use axnet1::*;
use starry_client::client::{FrameReader, now_us};

const DEFAULT_SERVER: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 5000;

fn send_frame(stream: &mut TcpStream, msg_type: u8, sequence: u32, payload: &[u8]) {
    let mut frame = vec![0u8; frame_len(payload.len())];
    encode_into(&mut frame, msg_type, 0, sequence, now_us(), 0, payload);
    let _ = stream.write_all(&frame);
}

/// An owned copy of a matched message (no borrows into the read buffer).
struct OwnedMsg {
    error_code: u32,
    payload: Vec<u8>,
}

/// Read frames until `predicate` matches, or `timeout` elapses.
fn wait_for<F>(stream: &mut TcpStream, timeout: Duration, mut predicate: F) -> Option<OwnedMsg>
where
    F: FnMut(&Message<'_>) -> bool,
{
    let mut reader = FrameReader::new();
    let mut buf = [0u8; 4096];
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        stream
            .set_read_timeout(Some(remaining.min(Duration::from_millis(100))))
            .ok()?;
        match stream.read(&mut buf) {
            Ok(0) => return None,
            Ok(n) => {
                reader.push(&buf[..n]);
                while let Some(frame) = reader.next_frame() {
                    if let Ok(msg) = decode(&frame, frame.len()) {
                        if predicate(&msg) {
                            return Some(OwnedMsg {
                                error_code: msg.error_code,
                                payload: msg.payload.to_vec(),
                            });
                        }
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return None,
        }
    }
}

fn main() {
    let mut failures = 0;
    let mut server = DEFAULT_SERVER.to_string();
    let mut port = DEFAULT_PORT;

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--server" => {
                i += 1;
                server = args[i].clone();
            }
            "--port" => {
                i += 1;
                port = args[i].parse().unwrap_or(port);
            }
            _ => {
                eprintln!("unknown arg: {}", args[i]);
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let addr = format!("{server}:{port}").parse::<SocketAddr>().unwrap();
    println!("[rtos-tester] connecting to {addr}");

    let mut stream = TcpStream::connect(addr).expect("connect failed");
    stream.set_nodelay(true).ok();

    // 1. Handshake: CONTROL(START) -> CONTROL_ACK.
    let hb_seq = 0x1000u32;
    send_frame(&mut stream, MSG_CONTROL, hb_seq, b"command=START");
    let ack = wait_for(&mut stream, Duration::from_secs(5), |m| {
        m.msg_type == MSG_CONTROL_ACK && m.sequence == hb_seq
    });
    if ack.is_some() {
        println!("[rtos-tester] PASS handshake CONTROL_ACK");
    } else {
        println!("[rtos-tester] FAIL handshake");
        failures += 1;
    }

    // 2. CONTROL with payload -> CONTROL_ACK.
    let c_seq = 0x2000u32;
    send_frame(&mut stream, MSG_CONTROL, c_seq, b"hello-axnet");
    let ack2 = wait_for(&mut stream, Duration::from_secs(5), |m| {
        m.msg_type == MSG_CONTROL_ACK && m.sequence == c_seq
    });
    if ack2.is_some() {
        println!("[rtos-tester] PASS control payload ack");
    } else {
        println!("[rtos-tester] FAIL control payload");
        failures += 1;
    }

    // 3. HEARTBEAT -> HEARTBEAT echo.
    let h_seq = 0x3000u32;
    send_frame(&mut stream, MSG_HEARTBEAT, h_seq, &[]);
    let echo = wait_for(&mut stream, Duration::from_secs(5), |m| {
        m.msg_type == MSG_HEARTBEAT && m.sequence == h_seq
    });
    if echo.is_some() {
        println!("[rtos-tester] PASS heartbeat echo");
    } else {
        println!("[rtos-tester] FAIL heartbeat");
        failures += 1;
    }

    // 4. Corrupt frame -> ERROR (bad CRC).
    {
        let mut frame = vec![0u8; frame_len(4)];
        encode_into(&mut frame, MSG_CONTROL, 0, 0x4000, now_us(), 0, b"data");
        frame[HEADER_LEN] ^= 0xFF; // corrupt payload -> CRC mismatch
        let _ = stream.write_all(&frame);
        let err = wait_for(&mut stream, Duration::from_secs(5), |m| {
            m.msg_type == MSG_ERROR
        });
        if let Some(e) = err {
            if e.error_code == ERR_BAD_CRC {
                println!(
                    "[rtos-tester] PASS bad-crc ERROR (code={:#x})",
                    e.error_code
                );
            } else {
                println!("[rtos-tester] FAIL bad-crc error code {:#x}", e.error_code);
                failures += 1;
            }
        } else {
            println!("[rtos-tester] FAIL no ERROR reply to corrupt frame");
            failures += 1;
        }
    }

    // 5. Periodic STATUS push (server sends every ~2 s).
    let status = wait_for(&mut stream, Duration::from_secs(6), |m| {
        m.msg_type == MSG_STATUS
    });
    if let Some(s) = status {
        let state = String::from_utf8_lossy(&s.payload);
        println!("[rtos-tester] PASS periodic STATUS (state={state})");
    } else {
        println!("[rtos-tester] FAIL no periodic STATUS push");
        failures += 1;
    }

    println!(
        "[rtos-tester] RESULT={}",
        if failures == 0 { "PASS" } else { "FAIL" }
    );
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
