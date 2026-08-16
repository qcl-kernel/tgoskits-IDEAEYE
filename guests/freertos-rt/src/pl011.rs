//! Minimal PL011 UART output driver (guest prints directly to the passed-through
//! physical UART; the host console shares the line, so lines are prefixed).

use super::PL011_BASE;

const UARTFR: usize = 0x18; // flag register
const UARTDR: usize = 0x00; // data register
const TXFF: u32 = 1 << 5; // transmit FIFO full

#[inline]
fn read_reg(off: usize) -> u32 {
    // Safety: MMIO read to the passed-through PL011.
    unsafe { core::ptr::read_volatile((PL011_BASE + off) as *const u32) }
}

#[inline]
fn write_reg(off: usize, val: u32) {
    // Safety: MMIO write to the passed-through PL011.
    unsafe { core::ptr::write_volatile((PL011_BASE + off) as *mut u32, val) }
}

#[inline]
pub fn putc(c: u8) {
    while read_reg(UARTFR) & TXFF != 0 {}
    write_reg(UARTDR, c as u32);
}

pub fn puts(s: &str) {
    for b in s.bytes() {
        if b == b'\n' {
            putc(b'\r');
        }
        putc(b);
    }
}

pub fn put_u64(mut v: u64) {
    if v == 0 {
        putc(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        putc(buf[n]);
    }
}
