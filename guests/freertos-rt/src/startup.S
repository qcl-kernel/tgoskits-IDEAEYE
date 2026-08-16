// FreeRTOS-style RT-probe guest for AxVisor (aarch64, EL1, MMU off).
//
// Layout (matches os/axvisor/configs/vms/qemu/aarch64/freertos-smp1.toml):
//   0x4000_0000  vectors (VBAR_EL1)
//   0x4000_1000  _start entry
//
// All vectors except the EL1h IRQ handler park in a loop. The EL1h IRQ
// handler (offset 0x280) saves the full register file + ELR/SPSR, calls
// `rt_irq_handler` (Rust), restores, and eret.

.section .vectors, "ax", @progbits
.balign 0x800
.global _vectors
_vectors:
    b _el1_sp0_sync
    .balign 0x80
    b _el1_sp0_irq
    .balign 0x80
    b _el1_sp0_fiq
    .balign 0x80
    b _el1_sp0_serror
    .balign 0x80
    b _el1_spx_sync
    .balign 0x80
    b _el1_spx_irq
    .balign 0x80
    b _el1_spx_fiq
    .balign 0x80
    b _el1_spx_serror
    .balign 0x80
    b _le_a64_sync
    .balign 0x80
    b _le_a64_irq
    .balign 0x80
    b _le_a64_fiq
    .balign 0x80
    b _le_a64_serror
    .balign 0x80
    b _le_a32_sync
    .balign 0x80
    b _le_a32_irq
    .balign 0x80
    b _le_a32_fiq
    .balign 0x80
    b _le_a32_serror
    .balign 0x800

.section .text.start, "ax", @progbits
.balign 4
.global _start
_start:
    // VBAR_EL1 = _vectors
    adrp x0, _vectors
    add  x0, x0, :lo12:_vectors
    msr  vbar_el1, x0

    // Exceptions at EL1 use SP_EL1.
    msr  spsel, #1

    // Zero BSS.
    adrp x0, __bss_start
    add  x0, x0, :lo12:__bss_start
    adrp x1, __bss_end
    add  x1, x1, :lo12:__bss_end
    cbz  x1, 2f
1:  str  xzr, [x0], #8
    cmp  x0, x1
    blt  1b
2:
    // Stack top.
    adrp x0, __stack_top
    add  x0, x0, :lo12:__stack_top
    mov  sp, x0

    // Enter Rust main (never returns).
    bl   rt_main
3:  wfi
    b    3b

.section .text.handlers, "ax", @progbits
.balign 4

.macro save_regs
    sub  sp, sp, #288
    stp  x0,  x1,  [sp, #0]
    stp  x2,  x3,  [sp, #16]
    stp  x4,  x5,  [sp, #32]
    stp  x6,  x7,  [sp, #48]
    stp  x8,  x9,  [sp, #64]
    stp  x10, x11, [sp, #80]
    stp  x12, x13, [sp, #96]
    stp  x14, x15, [sp, #112]
    stp  x16, x17, [sp, #128]
    stp  x18, x19, [sp, #144]
    stp  x20, x21, [sp, #160]
    stp  x22, x23, [sp, #176]
    stp  x24, x25, [sp, #192]
    stp  x26, x27, [sp, #208]
    stp  x28, x29, [sp, #224]
    str  x30, [sp, #240]
    mrs  x0, elr_el1
    str  x0, [sp, #248]
    mrs  x0, spsr_el1
    str  x0, [sp, #256]
.endm

.macro restore_regs
    ldr  x0, [sp, #256]
    msr  spsr_el1, x0
    ldr  x0, [sp, #248]
    msr  elr_el1, x0
    ldp  x0,  x1,  [sp, #0]
    ldp  x2,  x3,  [sp, #16]
    ldp  x4,  x5,  [sp, #32]
    ldp  x6,  x7,  [sp, #48]
    ldp  x8,  x9,  [sp, #64]
    ldp  x10, x11, [sp, #80]
    ldp  x12, x13, [sp, #96]
    ldp  x14, x15, [sp, #112]
    ldp  x16, x17, [sp, #128]
    ldp  x18, x19, [sp, #144]
    ldp  x20, x21, [sp, #160]
    ldp  x22, x23, [sp, #176]
    ldp  x24, x25, [sp, #192]
    ldp  x26, x27, [sp, #208]
    ldp  x28, x29, [sp, #224]
    ldr  x30, [sp, #240]
    add  sp, sp, #288
.endm

_el1_spx_irq:
    save_regs
    bl   rt_irq_handler
    restore_regs
    eret

// Every other vector: park.
_panic_loop:
    wfi
    b   _panic_loop

_el1_sp0_sync:   b _panic_loop
_el1_sp0_irq:    b _panic_loop
_el1_sp0_fiq:    b _panic_loop
_el1_sp0_serror: b _panic_loop
_el1_spx_sync:   b _panic_loop
_el1_spx_fiq:    b _panic_loop
_el1_spx_serror: b _panic_loop
_le_a64_sync:    b _panic_loop
_le_a64_irq:     b _panic_loop
_le_a64_fiq:     b _panic_loop
_le_a64_serror:  b _panic_loop
_le_a32_sync:    b _panic_loop
_le_a32_irq:     b _panic_loop
_le_a32_fiq:     b _panic_loop
_le_a32_serror:  b _panic_loop
