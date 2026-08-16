//! Feature-gated real-time instrumentation for AxVM (host side).
//!
//! Enabled by the `rt-instrument` feature (see docs/rt-axvisor). Off by default
//! with zero runtime cost. Provides:
//!
//! - host task-wakeup latency: from `queue_interrupt` to the vCPU run loop
//!   processing the queued interrupt (monotonic nanoseconds),
//! - an aggregate dump that also prints the `arm_vcpu` exit/entry counters on
//!   aarch64.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::host::{HostTime, default_host};

static WAKE_QUEUED_AT_NANOS: AtomicU64 = AtomicU64::new(0);
static WAKE_SAMPLES: AtomicU64 = AtomicU64::new(0);
static WAKE_SUM_NANOS: AtomicU64 = AtomicU64::new(0);
static WAKE_MIN_NANOS: AtomicU64 = AtomicU64::new(u64::MAX);
static WAKE_MAX_NANOS: AtomicU64 = AtomicU64::new(0);

#[inline]
fn now_nanos() -> u64 {
    default_host().monotonic_time().as_nanos() as u64
}

/// Called by `queue_interrupt` right before waking the target vCPU.
#[inline]
pub fn stamp_wake_queued() {
    WAKE_QUEUED_AT_NANOS.store(now_nanos(), Ordering::Relaxed);
}

/// Called from the vCPU run loop; folds the queued->processed latency once.
#[inline]
pub fn record_wake_latency() {
    let queued = WAKE_QUEUED_AT_NANOS.load(Ordering::Relaxed);
    if queued == 0 {
        return;
    }
    WAKE_QUEUED_AT_NANOS.store(0, Ordering::Relaxed);
    let delta = now_nanos().saturating_sub(queued);
    WAKE_SAMPLES.fetch_add(1, Ordering::Relaxed);
    WAKE_SUM_NANOS.fetch_add(delta, Ordering::Relaxed);
    fold_minmax(&WAKE_MIN_NANOS, &WAKE_MAX_NANOS, delta);
}

#[inline]
fn fold_minmax(min: &AtomicU64, max: &AtomicU64, v: u64) {
    let mut cur = min.load(Ordering::Relaxed);
    while v < cur {
        match min.compare_exchange_weak(cur, v, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => cur = actual,
        }
    }
    let mut cur = max.load(Ordering::Relaxed);
    while v > cur {
        match max.compare_exchange_weak(cur, v, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => cur = actual,
        }
    }
}

/// Prints the AxVM-side and (on aarch64) arm_vcpu-side RT counters.
pub fn dump() {
    let samples = WAKE_SAMPLES.load(Ordering::Relaxed);
    let avg = if samples > 0 { WAKE_SUM_NANOS.load(Ordering::Relaxed) / samples } else { 0 };
    info!(
        "RT-STATS: wakeup_samples={samples} min={} avg={avg} max={} (ns)",
        WAKE_MIN_NANOS.load(Ordering::Relaxed),
        WAKE_MAX_NANOS.load(Ordering::Relaxed),
    );
    #[cfg(target_arch = "aarch64")]
    arm_vcpu::rt_stats::dump();
}
