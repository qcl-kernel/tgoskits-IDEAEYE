//! `starry-client`: AXNET/1 TCP client for StarryOS.
//!
//! Connects to the FreeRTOS server (192.168.100.3:5000 by default), performs
//! the CONTROL handshake, then measures round-trip latency and throughput on
//! CONTROL requests while keeping the connection alive with HEARTBEATs.  When
//! the server drops, it reconnects with exponential backoff.
//!
//! Build a static musl binary for StarryOS's Alpine userspace:
//!   cargo build --target x86_64-unknown-linux-musl --release
//! The same binary also runs unmodified on a Linux host.

mod client;

use std::{env, net::Ipv4Addr, time::Duration};

use client::{Config, Stats, run};

fn usage(prog: &str) {
    eprintln!(
        "Usage: {prog} [options]\n\noptions:\n\x20 --server <ip>        RTOS server address \
         (default 192.168.100.3)\n\x20 --port <n>           RTOS server port   (default \
         5000)\n\x20 --requests <n>       CONTROL round-trips to complete (default 1000)\n\x20 \
         --payload <n>        payload bytes per CONTROL (default 1024, max 4096)\n\x20 \
         --heartbeat-ms <n>   heartbeat interval (default 1000)\n\x20 --timeout-ms <n>     \
         dead-connection threshold (default 3000)\n\x20 --duration <secs>    optional total run \
         bound (default: until requests done)\n\x20 --max-reconnect-ms <n> reconnect backoff cap \
         (default 10000)"
    );
}

fn parse_u64(name: &str, v: &str) -> u64 {
    v.parse().unwrap_or_else(|_| {
        eprintln!("invalid value for {name}: {v}");
        std::process::exit(2);
    })
}

fn print_stats(stats: &Stats) {
    println!("[starry-client] sent={}", stats.sent);
    println!("[starry-client] acked={}", stats.acked);
    println!("[starry-client] lost={}", stats.lost);
    println!("[starry-client] errors={}", stats.errors);
    println!("[starry-client] timeouts={}", stats.timeouts);
    println!("[starry-client] reconnects={}", stats.reconnects);
    println!("[starry-client] avg_rtt_us={}", stats.avg_rtt_us());
    println!("[starry-client] p50_rtt_us={}", stats.rtt_percentile(50.0));
    println!("[starry-client] p95_rtt_us={}", stats.rtt_percentile(95.0));
    println!("[starry-client] p99_rtt_us={}", stats.rtt_percentile(99.0));
    println!("[starry-client] max_rtt_us={}", stats.max_rtt_us());
    println!(
        "[starry-client] throughput_bytes={} throughput_us={} throughput_mbps={:.3}",
        stats.payload_bytes,
        stats.throughput_us,
        stats.throughput_bps() * 8.0 / 1_000_000.0
    );

    // Every transmitted request resolved (acked + lost) and no protocol errors.
    if stats.sent > 0 && stats.acked + stats.lost == stats.sent && stats.errors == 0 {
        println!("[starry-client] RESULT=PASS");
    } else {
        println!("[starry-client] RESULT=FAIL");
    }
}

fn main() {
    let mut server = "192.168.100.3".to_string();
    let mut port = 5000u16;
    let mut requests = 1000usize;
    let mut payload_len = 1024usize;
    let mut heartbeat_ms = 1000u64;
    let mut timeout_ms = 3000u64;
    let mut duration: Option<Duration> = None;
    let mut max_reconnect_ms = 10000u64;

    let args: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                usage(&args[0]);
                return;
            }
            "--server" => {
                i += 1;
                server = args.get(i).expect("--server needs a value").clone();
            }
            "--port" => {
                i += 1;
                port = parse_u64("--port", &args[i]) as u16;
            }
            "--requests" => {
                i += 1;
                requests = parse_u64("--requests", &args[i]) as usize;
            }
            "--payload" => {
                i += 1;
                payload_len = parse_u64("--payload", &args[i]) as usize;
            }
            "--heartbeat-ms" => {
                i += 1;
                heartbeat_ms = parse_u64("--heartbeat-ms", &args[i]);
            }
            "--timeout-ms" => {
                i += 1;
                timeout_ms = parse_u64("--timeout-ms", &args[i]);
            }
            "--duration" => {
                i += 1;
                duration = Some(Duration::from_secs(parse_u64("--duration", &args[i])));
            }
            "--max-reconnect-ms" => {
                i += 1;
                max_reconnect_ms = parse_u64("--max-reconnect-ms", &args[i]);
            }
            other => {
                eprintln!("unknown option: {other}");
                usage(&args[0]);
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let ip: Ipv4Addr = match server.parse() {
        Ok(ip) => ip,
        Err(_) => {
            eprintln!("invalid --server address: {server}");
            std::process::exit(2);
        }
    };

    let cfg = Config {
        server: ip,
        port,
        requests,
        payload_len,
        heartbeat_ms,
        timeout_ms,
        duration,
        max_reconnect_ms,
    };

    println!(
        "[starry-client] AXNET/1 client -> {}:{port} requests={requests} payload={payload_len}B \
         hb={heartbeat_ms}ms timeout={timeout_ms}ms",
        ip
    );

    let stats = run(&cfg, |s| {
        println!(
            "[starry-client] session sent={} acked={} errors={} timeouts={} reconnects={}",
            s.sent, s.acked, s.errors, s.timeouts, s.reconnects
        );
    });

    print_stats(&stats);
}
