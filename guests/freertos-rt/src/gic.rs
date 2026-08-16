//! Minimal GICv3 distributor/redistributor init for the physical GIC
//! (passthrough): enable the EL1 physical timer PPI (GIC ID 30) as a Group 1
//! interrupt delivered to EL1.

use super::{GICD_BASE, GICR_BASE};

const GICD_CTLR: usize = 0x0000;
const GICR_WAKER: usize = 0x0014;
const GICR_IGROUPR0: usize = 0x0080;
const GICR_ISENABLER0: usize = 0x0100;
const GICR_IPRIORITYR: usize = 0x0400;

#[inline]
fn read32(addr: usize) -> u32 {
    // Safety: MMIO read to the passed-through GIC.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

#[inline]
fn write32(addr: usize, val: u32) {
    // Safety: MMIO write to the passed-through GIC.
    unsafe { core::ptr::write_volatile(addr as *mut u32, val) }
}

/// Configure the physical GIC so the CNTP PPI is enabled and delivered at EL1.
pub fn init() {
    // Enable the distributor (Group 0 + Group 1). IRQs are still masked (DAIF.I=1)
    // until `rt_main` unmasks, so no interrupt can arrive mid-setup.
    write32(GICD_BASE + GICD_CTLR, read32(GICD_BASE + GICD_CTLR) | 0x3);

    // Wake the redistributor: clear ProcessorSleep, wait for ChildrenAsleep to drop.
    let waker = GICR_BASE + GICR_WAKER;
    write32(waker, read32(waker) & !(1 << 1));
    while (read32(waker) & (1 << 2)) != 0 {}

    // CNTP -> Group 1 (normal IRQ).
    write32(
        GICR_BASE + GICR_IGROUPR0,
        read32(GICR_BASE + GICR_IGROUPR0) | (1 << super::CNTP_GIC_ID),
    );

    // Priority 0xa0 for ID 30: IPRIORITYR7 covers IDs 28..31, byte (30 % 4) == 2.
    let prio = GICR_BASE + GICR_IPRIORITYR + 4 * 7;
    write32(prio, (read32(prio) & !(0xff << 16)) | (0xa0 << 16));

    // Enable PPI 30.
    write32(GICR_BASE + GICR_ISENABLER0, 1 << super::CNTP_GIC_ID);
}

/// Acknowledge the interrupt (reads and deactivates per GICv3 EOImode=0).
#[inline]
pub fn read_iar1() -> u64 {
    let v: u64;
    // Safety: ICC_IAR1_EL1 is accessible at EL1 (GICv3 sysreg CPU interface).
    unsafe { core::arch::asm!("mrs {0}, icc_iar1_el1", out(reg) v); }
    v
}

/// End of interrupt.
#[inline]
pub fn write_eoir1(id: u64) {
    // Safety: ICC_EOIR1_EL1 is accessible at EL1.
    unsafe { core::arch::asm!("msr icc_eoir1_el1, {0}", in(reg) id); }
}
