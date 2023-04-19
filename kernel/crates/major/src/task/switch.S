.altmacro
.macro SAVE_SN n
    sd s\n, (\n+2)*8(a0)
.endm
.macro LOAD_SN n
    ld s\n, (\n+2)*8(a1)
.endm
    .section .text
    .globl __switch
__switch:
    # __switch(
    #     current_task_ctx_ptr: *mut TaskContext,
    #     next_task_ctx_ptr: *const TaskContext
    # )
    # 保存当前任务的内核栈
    sd sp, 8(a0)
    # 保存 ra & s0~s11
    sd ra, 0(a0)
    .set n, 0
    .rept 12
        SAVE_SN %n
        .set n, n + 1
    .endr
    # 恢复下一个任务的 ra & s0~s11 寄存器
    ld ra, 0(a1)
    .set n, 0
    .rept 12
        LOAD_SN %n
        .set n, n + 1
    .endr
    # 恢复下一个任务的内核栈
    ld sp, 8(a1)
    ret

