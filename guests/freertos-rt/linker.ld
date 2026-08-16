OUTPUT_ARCH(aarch64)
ENTRY(_start)

MEMORY
{
    RAM (rwx) : ORIGIN = 0x40000000, LENGTH = 0x08000000
}

SECTIONS
{
    .vectors : ALIGN(0x800) { KEEP(*(.vectors)) } > RAM

    /* Entry at 0x4000_1000 (matches VM config `entry_point`); the gap between
     * the end of the vectors and the text is reserved. Addresses are supplied
     * via `--section-start` in .cargo/config.toml (rust-lld honors those). */
    .text : ALIGN(16) {
        *(.text.start)
        *(.text.handlers)
        *(.text .text.*)
    } > RAM

    .rodata : ALIGN(16) { *(.rodata .rodata.*) } > RAM
    .data : ALIGN(16) { *(.data .data.*) } > RAM

    . = ALIGN(16);
    __bss_start = .;
    .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) } > RAM
    __bss_end = .;

    . = ALIGN(16);
    .stack (NOLOAD) : {
        __stack_bottom = .;
        . += 0x20000;              /* 128 KiB kernel stack */
        __stack_top = .;
    } > RAM

    /* Shared RT-stats page at a fixed GPA the hypervisor can read back.
     * NOLOAD: not part of the flat image (RAM is zeroed at boot; the guest
     * initializes the page itself), it only reserves the region. */
    .stats (NOLOAD) : ALIGN(16) { KEEP(*(.stats)) } > RAM

    /DISCARD/ : { *(.ARM.attributes) *(.comment) *(.eh_frame) *(.got .got.*) }
}
