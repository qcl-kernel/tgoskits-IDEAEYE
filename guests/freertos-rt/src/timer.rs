//! EL1 physical timer (CNTP) access. In passthrough mode the hypervisor has
//! already set CNTHCTL_EL2.EL1PCEN|EL1PCTEN, so the guest programs CNTP freely.

/// Timer frequency in Hz (QEMU virt aarch64 exposes this via CNTFRQ_EL0).
#[inline]
pub fn read_cntfrq() -> u64 {
    let v: u64;
    // Safety: CNTFRQ_EL0 is EL0/EL1 accessible.
    unsafe { core::arch::asm!("mrs {0}, cntfrq_el0", out(reg) v); }
    v
}

/// Physical counter value (used as the cycle clock for latency stats).
#[inline]
pub fn read_cntpct() -> u64 {
    let v: u64;
    // Safety: CNTPCT_EL0 is EL0/EL1 accessible.
    unsafe { core::arch::asm!("mrs {0}, cntpct_el0", out(reg) v); }
    v
}

/// Arm a one-shot CNTP deadline `period_ns` from now.
#[inline]
pub fn arm_cntp_oneshot(period_ns: u64) {
    let freq = read_cntfrq();
    let ticks = (freq as u128 * period_ns as u128 / 1_000_000_000) as u64;
    // Safety: CNTP_TVAL_EL0 / CNTP_CTL_EL0 are EL0/EL1 accessible.
    unsafe {
        core::arch::asm!("msr cntp_tval_el0, {0}", in(reg) ticks);
        // IMASK=0 (interrupts enabled), ENABLE=1.
        core::arch::asm!("msr cntp_ctl_el0, {0}", in(reg) 1u64);
    }
}
