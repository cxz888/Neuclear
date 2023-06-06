    .section .text.entry
    .globl _start
_start:
    la sp, boot_stack_top
    call __set_boot_pt
    la   t0, rust_main
    li   t1, 0xffffffff00000000
    add  t0, t0, t1
    add  sp, sp, t1
    jr   t0

__set_boot_pt:
    la   t0, boot_pt
    srli t0, t0, 12
    li   t1, 8 << 60
    or   t0, t0, t1
    csrw satp, t0
    ret

    .section .bss.stack
    .align 12
    .globl boot_stack
boot_stack:
    .space 4096 * 16
    .globl boot_stack_top
boot_stack_top:

    .section .data
    .align 12
boot_pt:
    // 0x0000_0000_8000_0000 -> 0x8000_0000 (1G, VRWXAD)
    // 0xffff_ffff_8000_0000 -> 0x8000_0000 (1G, VRWXAD)
    .zero 2 * 8                     // [0][1]
    .8byte (0x80000 << 10) | 0xcf   // [2]
    .zero 507 * 8                   // [3]~[509]
    .8byte (0x80000 << 10) | 0xcf   // [510]
    .zero 1 * 8                     // [511]
