//! FreeRTOS-style RT-probe guest for the AxVisor hypervisor (aarch64, EL1, MMU off).
//!
//! This is a minimal bare-metal "RT probe" used to measure deterministic latency
//! characteristics of AxVisor. It deliberately is NOT the full FreeRTOS kernel; it
//! exercises the same primitives a FreeRTOS AArch64 port would use:
//!
//! - an EL1 exception vector table (VBAR_EL1 at `0x4000_0000`),
//! - the EL1 physical timer CNTP as a periodic tick,
//! - the physical GIC (GICv3) PPI for the timer,
//! - a tiny cooperative task model (interrupt handler -> high-priority reporter),
//! - a shared RT-stats page the hypervisor can read back at a fixed GPA.
//!
//! Dropping in the real FreeRTOS kernel later is a drop-in change: the memory map,
//! vector base, entry point and hypervisor-facing conventions stay identical.
//!
//! Memory map (matches `freertos-smp1.toml` + guest DTB):
//! ```text
//! 0x4000_0000  vectors (VBAR_EL1)            (also kernel load base)
//! 0x4000_1000  _start (VM config `entry_point`)
//! 0x4000_2000  .text / .rodata / .data / .bss / .stack
//! 0x4070_0000  shared RT-stats page (`RtStats`)
//! 0x4080_0000  end of guest RAM (128 MiB)
//! 0x0800_0000  GICD, 0x080A_0000 GICR (CPU0), 0x0900_0000 PL011 UART
//! ```

#![no_std]
#![no_main]

use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

mod gic;
mod pl011;
mod timer;

global_asm!(include_str!("startup.S"));

// ---------------------------------------------------------------------------
// Fixed guest memory map
// ---------------------------------------------------------------------------

/// Kernel load base / VBAR_EL1.
pub const KERNEL_BASE: usize = 0x4000_0000;
/// Entry point, must match the VM config `entry_point`.
pub const ENTRY: usize = 0x4000_1000;
/// End of guest RAM (0x4000_0000 + 128 MiB, MapAlloc).
pub const RAM_END: usize = 0x4080_0000;
/// Fixed GPA of the shared RT-stats page (guest-owned RAM).
pub const STATS_GPA: usize = 0x4070_0000;

pub const PL011_BASE: usize = 0x0900_0000;
pub const GICD_BASE: usize = 0x0800_0000;
/// CPU0 redistributor (qemu virt GICv3).
pub const GICR_BASE: usize = 0x080A_0000;

/// EL1 physical timer (CNTP): PPI 14 -> GIC interrupt ID 30.
pub const CNTP_GIC_ID: u32 = 16 + 14;
/// Tick period.
pub const TICK_NS: u64 = 1_000_000;
/// After this many ticks (~10 s), the probe issues PSCI SYSTEM_OFF so the
/// guest exits and the hypervisor prints its RT-STATS dump.
pub const SHUTDOWN_AFTER_TICKS: u64 = 10_000;

// ---------------------------------------------------------------------------
// Shared RT-stats page (host reads via `read_from_guest` at STATS_GPA)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtStats {
    pub magic: u64, // 0x52545052_0000_0001 ("RTPR" v1)
    pub samples: u64,
    pub latency_min: u64,
    pub latency_max: u64,
    pub latency_sum: u64,
    pub irq_entry_cycles: u64,
    pub handler_exec_sum: u64,
    pub handler_exec_max: u64,
    pub exits: u64,
    pub ticks: u64,
    /// Coarse histogram of IRQ->task latency, 64 cycles per bucket.
    pub hist: [u64; 128],
}

#[link_section = ".stats"]
pub static mut RT_STATS: RtStats = RtStats {
    magic: 0x52545052_0000_0001,
    samples: 0,
    latency_min: u64::MAX,
    latency_max: 0,
    latency_sum: 0,
    irq_entry_cycles: 0,
    handler_exec_sum: 0,
    handler_exec_max: 0,
    exits: 0,
    ticks: 0,
    hist: [0; 128],
};

/// Raw pointer to the shared stats page (placed at `STATS_GPA` by the linker).
///
/// Access goes through raw pointers rather than `static mut` references to keep
/// the single-core, IRQ-context-plus-main-loop sharing model explicit.
fn stats_mut() -> *mut RtStats {
    core::ptr::addr_of_mut!(RT_STATS)
}

// Atomics shared between the IRQ handler and the main loop.
static IRQ_ENTRY: AtomicU64 = AtomicU64::new(0);
static IRQ_HANDLER_END: AtomicU64 = AtomicU64::new(0);
static IRQ_SEEN: AtomicBool = AtomicBool::new(false);

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        // Safety: parking in a loop.
        unsafe { core::arch::asm!("wfi"); }
    }
}

/// Called from the EL1h IRQ vector stub with IRQs masked.
#[no_mangle]
extern "C" fn rt_irq_handler() {
    let iar = gic::read_iar1();
    let id = iar & 0x3ff;
    let now = timer::read_cntpct();

    if id == CNTP_GIC_ID as u64 {
        IRQ_ENTRY.store(now, Ordering::SeqCst);
        unsafe { (*stats_mut()).irq_entry_cycles = now; }
        // Re-arm the one-shot CNTP for the next tick.
        timer::arm_cntp_oneshot(TICK_NS);
        IRQ_SEEN.store(true, Ordering::SeqCst);
        unsafe { (*stats_mut()).ticks = (*stats_mut()).ticks.wrapping_add(1); }
    }
    // Safety: EOI is required for level-triggered PPIs; always EOIR.
    gic::write_eoir1(id);

    let end = timer::read_cntpct();
    IRQ_HANDLER_END.store(end, Ordering::SeqCst);
    let exec = end.wrapping_sub(now);
    unsafe {
        (*stats_mut()).handler_exec_sum = (*stats_mut()).handler_exec_sum.wrapping_add(exec);
        if exec > (*stats_mut()).handler_exec_max {
            (*stats_mut()).handler_exec_max = exec;
        }
    }
}

/// Rust entry point, called from `_start` (stack/BSS/vectors already set up).
#[no_mangle]
extern "C" fn rt_main() -> ! {
    // The `.stats` page is NOLOAD (not part of the image); re-assert its magic
    // now that RAM is live so the hypervisor can recognize it.
    unsafe { (*stats_mut()).magic = 0x52545052_0000_0001; }

    pl011::puts("\r\n[RT] probe started, entry=0x4000_1000, stats=0x4070_0000\r\n");

    // Initialize GIC + timer with IRQs masked, then unmask IRQ.
    unsafe {
        gic::init();
        timer::arm_cntp_oneshot(TICK_NS);
        // Unmask IRQ (PSTATE.I).
        core::arch::asm!("msr daifclr, #0x2");
    }

    let mut tick: u64 = 0;
    loop {
        // Sleep until the next CNTP tick (or any IRQ).
        // Safety: WFI is safe in a loop; the IRQ will be handled by the vector table.
        unsafe { core::arch::asm!("wfi", options(nomem, nostack)); }

        if IRQ_SEEN.swap(false, Ordering::SeqCst) {
            tick = tick.wrapping_add(1);

            // IRQ -> task latency: time from CNTP IRQ entry to the reporter
            // task observing the flag (the dominant component of interrupt-to-task
            // response in this minimal model).
            let now = timer::read_cntpct();
            let entry = IRQ_ENTRY.load(Ordering::SeqCst);
            let delta = now.wrapping_sub(entry);
            record_latency(delta);

            // Periodic host-exit provocation: an hvc that traps to EL2. The
            // hypervisor reads the call number from x0 (not the hvc immediate),
            // so set x0 to 0xFFFF — not a valid AxVisor hypercall nor a PSCI
            // function — to get a harmless "Invalid hypercall code" warn.
            if tick % 2000 == 0 {
                // Safety: hvc traps to EL2. The call number is read from x0
                // (not the hvc immediate); 0xFFFF is neither a valid AxVisor
                // hypercall nor a PSCI function, so the host logs a harmless
                // "Invalid hypercall code" warn.
                unsafe {
                    core::arch::asm!(
                        "hvc #0",
                        in("x0") 0xFFFFu64,
                        options(nostack)
                    );
                }
                unsafe { (*stats_mut()).exits = (*stats_mut()).exits.wrapping_add(1); }
            }
            if tick % 2000 == 0 {
                print_stats(tick);
            }

            // After ~10 s, shut the VM down so the hypervisor prints its
            // RT-STATS counters (host dump happens on the last vCPU exit).
            if tick >= SHUTDOWN_AFTER_TICKS {
                pl011::puts("[RT] issuing PSCI SYSTEM_OFF\r\n");
                // Safety: PSCI SYSTEM_OFF (0x8400_0008) via hvc traps to EL2
                // and should never return; park if it somehow does.
                unsafe {
                    core::arch::asm!(
                        "hvc #0",
                        in("x0") 0x8400_0008u64,
                        options(nostack)
                    );
                }
                loop {
                    // Safety: parking loop.
                    unsafe { core::arch::asm!("wfi", options(nomem, nostack)); }
                }
            }
        }
    }
}

fn record_latency(delta: u64) {
    // Safety: single CPU, IRQ context + main loop access the stats page; the host
    // only reads it, never writes, so unsynchronized updates are safe here.
    unsafe {
        let s = &mut *stats_mut();
        s.samples = s.samples.wrapping_add(1);
        if delta < s.latency_min {
            s.latency_min = delta;
        }
        if delta > s.latency_max {
            s.latency_max = delta;
        }
        s.latency_sum = s.latency_sum.wrapping_add(delta);
        let bucket = (delta >> 6) as usize;
        if bucket < s.hist.len() {
            s.hist[bucket] = s.hist[bucket].wrapping_add(1);
        }
    }
}

fn print_stats(tick: u64) {
    // Safety: read-only snapshot.
    unsafe {
        let s = &*stats_mut();
        let avg = if s.samples > 0 { s.latency_sum / s.samples } else { 0 };
        pl011::puts("[RT] ticks=");
        pl011::put_u64(tick);
        pl011::puts(" samples=");
        pl011::put_u64(s.samples);
        pl011::puts(" irq2task min=");
        pl011::put_u64(s.latency_min);
        pl011::puts(" avg=");
        pl011::put_u64(avg);
        pl011::puts(" max=");
        pl011::put_u64(s.latency_max);
        pl011::puts(" handler_max=");
        pl011::put_u64(s.handler_exec_max);
        pl011::puts(" exits=");
        pl011::put_u64(s.exits);
        pl011::puts("\r\n");
    }
}
