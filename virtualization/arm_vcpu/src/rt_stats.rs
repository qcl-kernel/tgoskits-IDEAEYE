//! Feature-gated real-time instrumentation for the AArch64 vCPU core.
//!
//! Enabled by the `rt-instrument` feature (see docs/rt-axvisor). Off by default
//! with zero runtime cost. Provides:
//!
//! - per-exit-reason counters,
//! - guest-entry preparation cost (`restore_vm_system_regs`: sysreg restore + cache/TLB flush),
//! - exit-handling cost (`vmexit_handler`: sysreg store + decode),
//!
//! using the physical counter (`CNTPCT_EL0`) as a cycle clock. All counters are
//! relaxed atomics so they are safe from both EL2 IRQ context and task context.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::ArmVmExit;

/// Index of an `ArmVmExit` variant in the reason histogram.
const fn exit_index(exit: &ArmVmExit) -> usize {
    match exit {
        ArmVmExit::Hypercall { .. } => 0,
        ArmVmExit::MmioRead { .. } => 1,
        ArmVmExit::MmioWrite { .. } => 2,
        ArmVmExit::SysRegRead { .. } => 3,
        ArmVmExit::SysRegWrite { .. } => 4,
        ArmVmExit::ExternalInterrupt { .. } => 5,
        ArmVmExit::CpuDown { .. } => 6,
        ArmVmExit::CpuUp { .. } => 7,
        ArmVmExit::SystemDown => 8,
        ArmVmExit::SendIPI { .. } => 9,
        ArmVmExit::Nothing => 10,
        // Exhaustive: `ArmVmExit` is `#[non_exhaustive]`, but this match is in
        // the same crate, so adding a variant here will surface as a compile
        // error prompting the index table to be extended.
    }
}

pub const REASON_COUNT: usize = 12;

static EXIT_COUNT: AtomicU64 = AtomicU64::new(0);
static ENTRY_CYCLES_SUM: AtomicU64 = AtomicU64::new(0);
static ENTRY_CYCLES_MIN: AtomicU64 = AtomicU64::new(u64::MAX);
static ENTRY_CYCLES_MAX: AtomicU64 = AtomicU64::new(0);
static EXIT_CYCLES_SUM: AtomicU64 = AtomicU64::new(0);
static EXIT_CYCLES_MIN: AtomicU64 = AtomicU64::new(u64::MAX);
static EXIT_CYCLES_MAX: AtomicU64 = AtomicU64::new(0);
static REASON_COUNTS: [AtomicU64; REASON_COUNT] = [const { AtomicU64::new(0) }; REASON_COUNT];

/// Reads the physical counter (cycle clock). Works at EL2/EL1.
#[inline]
pub fn cntpct() -> u64 {
    let v: u64;
    // Safety: CNTPCT_EL0 is accessible at every exception level.
    unsafe { core::arch::asm!("mrs {0}, cntpct_el0", out(reg) v); }
    v
}

/// Records one guest exit and its reason.
#[inline]
pub fn record_exit(exit: &ArmVmExit) {
    EXIT_COUNT.fetch_add(1, Ordering::Relaxed);
    REASON_COUNTS[exit_index(exit)].fetch_add(1, Ordering::Relaxed);
}

/// Records the guest-entry preparation cost (sysreg restore + cache/TLB flush).
#[inline]
pub fn record_entry_cycles(cycles: u64) {
    ENTRY_CYCLES_SUM.fetch_add(cycles, Ordering::Relaxed);
    fold_minmax(&ENTRY_CYCLES_MIN, &ENTRY_CYCLES_MAX, cycles);
}

/// Records the exit-handling cost (sysreg store + decode).
#[inline]
pub fn record_exit_cycles(cycles: u64) {
    EXIT_CYCLES_SUM.fetch_add(cycles, Ordering::Relaxed);
    fold_minmax(&EXIT_CYCLES_MIN, &EXIT_CYCLES_MAX, cycles);
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

const REASON_NAMES: [&str; REASON_COUNT] = [
    "hypercall",
    "mmio_read",
    "mmio_write",
    "sysreg_read",
    "sysreg_write",
    "ext_irq",
    "cpu_down",
    "cpu_up",
    "system_down",
    "send_ipi",
    "nothing",
    "other",
];

/// Prints a human-readable summary (called when the VM stops).
pub fn dump() {
    let count = EXIT_COUNT.load(Ordering::Relaxed);
    let entry_avg = if count > 0 { ENTRY_CYCLES_SUM.load(Ordering::Relaxed) / count } else { 0 };
    let exit_avg = if count > 0 { EXIT_CYCLES_SUM.load(Ordering::Relaxed) / count } else { 0 };
    info!(
        "RT-STATS: exits={count} | entry_cycles min={} avg={entry_avg} max={} | exit_cycles min={} avg={exit_avg} max={}",
        ENTRY_CYCLES_MIN.load(Ordering::Relaxed),
        ENTRY_CYCLES_MAX.load(Ordering::Relaxed),
        EXIT_CYCLES_MIN.load(Ordering::Relaxed),
        EXIT_CYCLES_MAX.load(Ordering::Relaxed),
    );
    for (i, name) in REASON_NAMES.iter().enumerate() {
        let c = REASON_COUNTS[i].load(Ordering::Relaxed);
        if c > 0 {
            info!("RT-STATS: exit[{name}] = {c}");
        }
    }
}
