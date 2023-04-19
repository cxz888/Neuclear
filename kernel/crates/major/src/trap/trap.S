.altmacro
.macro SAVE_GP n
    sd x\n, \n*8(sp)
.endm
.macro LOAD_GP n
    ld x\n, \n*8(sp)
.endm
    .section .text.trampoline
    .globl __alltraps
    .globl __restore
    .align 2
__alltraps:
    csrrw sp, sscratch, sp
    # 现在 sp 指向该线程的 TrapContext，也就是内核栈的栈顶，sscratch 指向用户栈顶
    # 保存通用寄存器
    sd x1, 1*8(sp)
    # 跳过 sp(x2)，待会会保存
    sd x3, 3*8(sp)
    # 跳过 tp(x4)，暂时用不到，后续可能有用
    # 保存 x5~x31
    .set n, 5
    .rept 27
        SAVE_GP %n
        .set n, n+1
    .endr
    # 现在可以自由使用 t0/t1/t2，因为它们已经保存在 TrapContext 中了
    csrr t0, sstatus
    csrr t1, sepc
    sd t0, 32*8(sp)
    sd t1, 33*8(sp)
    # 这时才保存 sp，因为 sp 刚开始交换到 sscratch 里了
    csrr t2, sscratch
    sd t2, 2*8(sp)
    call __trap_handler

__restore:
    # 现在 sp 指向内核栈的 TrapContext 上
    # 保存到 sscratch，以便下次 trap 时使用
    csrw sscratch, sp
    # 恢复 sstatus/sepc
    ld t0, 32*8(sp)
    ld t1, 33*8(sp)
    csrw sstatus, t0
    csrw sepc, t1
    # 恢复通用寄存器，除了 x0/sp/tp
    ld x1, 1*8(sp)
    ld x3, 3*8(sp)
    .set n, 5
    .rept 27
        LOAD_GP %n
        .set n, n+1
    .endr
    # 切回用户栈
    ld sp, 2*8(sp)
    sret
