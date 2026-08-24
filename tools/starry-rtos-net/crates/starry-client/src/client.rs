//! AXNET/1 TCP client core, shared by the StarryOS binary, the host tester
//! and the loopback tests.
//!
//! Connects to the server, does a CONTROL(START) handshake, then drives
//! CONTROL round-trips and HEARTBEATs.  It times each RTT, tallies success /
//! error / timeout / reconnect statistics, and reconnects with exponential
//! backoff when the session dies.
use std::{
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    time::{Duration, Instant},
};

use axnet1::*;

#[derive(Debug, Clone)]
pub struct Config {
    pub server: Ipv4Addr,
    pub port: u16,
    /// Number of CONTROL round-trips to complete.
    pub requests: usize,
    /// Payload bytes of each CONTROL message.
    pub payload_len: usize,
    /// Heartbeat interval.
    pub heartbeat_ms: u64,
    /// A session is dead if no frame arrives within this window.
    pub timeout_ms: u64,
    /// Wall-clock bound; `None` runs until the requests finish.
    pub duration: Option<Duration>,
    /// Reconnect backoff doubles from 1 s up to this cap.
    pub max_reconnect_ms: u64,
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    pub sent: usize,
    pub acked: usize,
    /// In-flight requests lost when a session broke before their ACK arrived.
    pub lost: usize,
    pub errors: usize,
    pub timeouts: usize,
    pub reconnects: usize,
    pub rtts_us: Vec<u64>,
    /// Payload bytes transmitted for the throughput phase.
    pub payload_bytes: u64,
    /// Wall time of the throughput phase, from first request to last ack.
    pub throughput_us: u64,
}

impl Stats {
    pub fn rtt_percentile(&self, pct: f64) -> u64 {
        let mut v = self.rtts_us.clone();
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        let idx = ((pct / 100.0) * (v.len() - 1) as f64).round() as usize;
        v[idx]
    }
    pub fn avg_rtt_us(&self) -> u64 {
        if self.rtts_us.is_empty() {
            0
        } else {
            self.rtts_us.iter().sum::<u64>() / self.rtts_us.len() as u64
        }
    }
    pub fn max_rtt_us(&self) -> u64 {
        self.rtts_us.iter().copied().max().unwrap_or(0)
    }
    /// Effective application throughput in bytes/second (payload only).
    pub fn throughput_bps(&self) -> f64 {
        if self.throughput_us == 0 {
            0.0
        } else {
            self.payload_bytes as f64 * 1_000_000.0 / self.throughput_us as f64
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Broken,
}

/// Accumulates a TCP byte stream into complete AXNET/1 frames.
#[derive(Default)]
pub struct FrameReader {
    buf: Vec<u8>,
}

impl FrameReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        // A stream that never yields a valid header is corrupt; drop it rather
        // than let it grow without bound.
        if self.buf.len() > FRAME_MAX + 4096 {
            self.buf.clear();
        }
    }

    /// Extract the next complete frame, if any.
    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        if self.buf.len() < HEADER_LEN {
            return None;
        }
        let plen = u32::from_be_bytes(self.buf[6..10].try_into().unwrap()) as usize;
        if plen > MAX_PAYLOAD {
            self.buf.clear(); // cannot trust the stream anymore
            return None;
        }
        let flen = frame_len(plen);
        if self.buf.len() < flen {
            return None;
        }
        let frame = self.buf[..flen].to_vec();
        self.buf.drain(..flen);
        Some(frame)
    }
}

fn send_frame(
    stream: &mut TcpStream,
    msg_type: u8,
    sequence: u32,
    payload: &[u8],
) -> io::Result<()> {
    let mut frame = vec![0u8; frame_len(payload.len())];
    encode_into(&mut frame, msg_type, 0, sequence, now_us(), 0, payload);
    stream.write_all(&frame)
}

/// Wall-clock microseconds since the Unix epoch, used for the wire timestamp.
pub fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Connect and perform the AXNET/1 CONTROL(START) handshake.
pub fn connect_and_handshake(cfg: &Config, seq: &mut u32) -> io::Result<TcpStream> {
    let addr = SocketAddr::new(cfg.server.into(), cfg.port);
    let mut stream = TcpStream::connect(addr)?;
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(Duration::from_millis(cfg.timeout_ms)))?;

    let my_seq = *seq;
    *seq = seq.wrapping_add(1);
    send_frame(&mut stream, MSG_CONTROL, my_seq, b"command=START")?;

    let mut reader = FrameReader::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "peer closed during handshake",
            ));
        }
        reader.push(&buf[..n]);
        while let Some(frame) = reader.next_frame() {
            if let Ok(msg) = decode(&frame, frame.len()) {
                if msg.msg_type == MSG_CONTROL_ACK && msg.sequence == my_seq {
                    return Ok(stream);
                }
            }
        }
    }
}

/// Count a broken session.  An in-flight request can never be acked, so it
/// counts as `lost`.
fn mark_broken(stats: &mut Stats, in_flight: &Option<(u32, Instant)>, is_timeout: bool) -> Outcome {
    if in_flight.is_some() {
        stats.lost += 1;
    }
    if is_timeout {
        stats.timeouts += 1;
    }
    Outcome::Broken
}

/// Drive one connected session until it breaks or completes.
pub fn run_session(
    stream: &mut TcpStream,
    cfg: &Config,
    seq: &mut u32,
    stats: &mut Stats,
    session_start: Instant,
) -> Outcome {
    if stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .is_err()
    {
        return mark_broken(stats, &None, false);
    }

    let mut reader = FrameReader::new();
    let mut last_rx = Instant::now();
    let mut last_hb = Instant::now();
    let mut in_flight: Option<(u32, Instant)> = None;
    let mut throughput_start: Option<Instant> = None;
    let mut buf = [0u8; 8192];

    loop {
        let now = Instant::now();

        // Heartbeat.
        if now.duration_since(last_hb) >= Duration::from_millis(cfg.heartbeat_ms) {
            let hseq = *seq;
            *seq = seq.wrapping_add(1);
            if send_frame(stream, MSG_HEARTBEAT, hseq, &[]).is_err() {
                return mark_broken(stats, &in_flight, false);
            }
            last_hb = now;
        }

        // Next request; one in flight keeps RTT measurement simple.
        if stats.sent < cfg.requests && in_flight.is_none() {
            let plen = cfg.payload_len.min(MAX_PAYLOAD);
            let payload = vec![0xA5u8; plen]; // deterministic fill
            let rseq = *seq;
            *seq = seq.wrapping_add(1);
            if send_frame(stream, MSG_CONTROL, rseq, &payload).is_err() {
                return mark_broken(stats, &in_flight, false);
            }
            stats.sent += 1;
            stats.payload_bytes += plen as u64;
            if throughput_start.is_none() {
                throughput_start = Some(now);
            }
            in_flight = Some((rseq, now));
        }

        // Read and process incoming frames.
        match stream.read(&mut buf) {
            Ok(0) => return mark_broken(stats, &in_flight, false), // peer closed
            Ok(n) => {
                reader.push(&buf[..n]);
                while let Some(frame) = reader.next_frame() {
                    match decode(&frame, frame.len()) {
                        Ok(msg) => {
                            last_rx = Instant::now();
                            match msg.msg_type {
                                MSG_CONTROL_ACK => {
                                    if let Some((rseq, sent_at)) = in_flight {
                                        if rseq == msg.sequence {
                                            stats.acked += 1;
                                            stats
                                                .rtts_us
                                                .push(sent_at.elapsed().as_micros() as u64);
                                            if let Some(t0) = throughput_start {
                                                stats.throughput_us =
                                                    t0.elapsed().as_micros() as u64;
                                            }
                                            in_flight = None;
                                        }
                                    }
                                }
                                MSG_HEARTBEAT => { /* liveness only */ }
                                MSG_STATUS => { /* server push */ }
                                MSG_ERROR => stats.errors += 1,
                                _ => {}
                            }
                        }
                        Err(_) => stats.errors += 1,
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return mark_broken(stats, &in_flight, false),
        }

        // Give up if no frame arrived within `timeout_ms`.
        if last_rx.elapsed() >= Duration::from_millis(cfg.timeout_ms) {
            return mark_broken(stats, &in_flight, true);
        }

        // Done once every transmitted request is resolved (acked + lost).
        if stats.sent >= cfg.requests && stats.acked + stats.lost >= stats.sent {
            return Outcome::Done;
        }
        if let Some(d) = cfg.duration {
            if session_start.elapsed() >= d {
                return Outcome::Done;
            }
        }
    }
}

/// Top-level run loop with reconnect + exponential backoff.
pub fn run(cfg: &Config, mut on_session: impl FnMut(&Stats)) -> Stats {
    let mut stats = Stats::default();
    let mut seq: u32 = 0;
    let session_start = Instant::now();
    let mut backoff_ms = 1000u64;

    loop {
        match connect_and_handshake(cfg, &mut seq) {
            Ok(mut stream) => {
                backoff_ms = 1000;
                let outcome = run_session(&mut stream, cfg, &mut seq, &mut stats, session_start);
                on_session(&stats);
                match outcome {
                    Outcome::Done => break,
                    Outcome::Broken => {
                        stats.reconnects += 1;
                    }
                }
            }
            Err(_) => {
                stats.errors += 1;
            }
        }

        if stats.sent >= cfg.requests && stats.acked + stats.lost >= stats.sent {
            break;
        }
        if let Some(d) = cfg.duration {
            if session_start.elapsed() >= d {
                break;
            }
        }

        // Reconnect backoff: 1, 2, 4, 8, ... capped.
        std::thread::sleep(Duration::from_millis(backoff_ms));
        backoff_ms = (backoff_ms * 2).min(cfg.max_reconnect_ms);
    }

    stats
}
