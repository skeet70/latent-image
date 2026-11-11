.syntax unified
.arm
.arch armv7-a
.text
.align 2
.global xm1_power_standby_task
.type xm1_power_standby_task,%function
xm1_power_standby_task:
    push {r4,r5,r6,r11,lr}
    add r11,sp,#16
    sub sp,sp,#12
1:
    mov r0,#1
    sub r1,r11,#24
    mvn r2,#0
    bl xm1_power_wait_current_any
    ldr r3,[r11,#-24]
    cmp r3,#1
    bne 1b
    bl xm1_power_save_irq
    mov r6,r0
    bl xm1_power_whole_cache
    mov r0,#4
    bl xm1_power_cache_disable
    mov r0,#0x1000
    bl xm1_power_cache_disable
    ldr r4,=0xfffb4080
    ldm r4,{r0,r1,r2,r12}
    mvn r3,#0
    str r3,[r4,#0x20]
    str r3,[r4,#0x24]
    str r3,[r4,#0x28]
    str r3,[r4,#0x2c]
    mov r5,#0x01000000
    str r5,[r4,#12]
    wfi
    str r5,[r4,#0x2c]
    stm r4,{r0,r1,r2,r12}
    mov r0,r6
    bl xm1_power_restore_irq
    mov r0,#0x1000
    bl xm1_power_cache_enable
    mov r0,#4
    bl xm1_power_cache_enable
    b 1b
.size xm1_power_standby_task,.-xm1_power_standby_task

.global xm1_power_save_irq
.type xm1_power_save_irq,%function
xm1_power_save_irq:
    mrs r1,cpsr
    orr r2,r1,#0x80
    msr cpsr_c,r2
    and r0,r1,#0x80
    bx lr
.global xm1_power_restore_irq
.type xm1_power_restore_irq,%function
xm1_power_restore_irq:
    and r1,r0,#0x80
    mrs r2,cpsr
    bic r2,r2,#0x80
    orr r1,r1,r2
    msr cpsr_c,r1
    bx lr

.global xm1_power_cache_disable
.type xm1_power_cache_disable,%function
xm1_power_cache_disable:
    mrc p15,0,r1,c1,c0,0
    bic r1,r1,r0
    dsb sy
    mcr p15,0,r1,c1,c0,0
    isb sy
    bx lr
.global xm1_power_cache_enable
.type xm1_power_cache_enable,%function
xm1_power_cache_enable:
    mrc p15,0,r2,c1,c0,0
    orr r2,r2,r0
    mov r1,#0
    dsb sy
    tst r0,#0x1000
    mcreq p15,0,r1,c7,c5,0
    tst r0,#4
    mcreq p15,0,r1,c15,c5,0
    mcr p15,0,r2,c1,c0,0
    isb sy
    bx lr

.global xm1_power_whole_cache
.type xm1_power_whole_cache,%function
xm1_power_whole_cache:
    mov r2,#1
    b .Lwhole_cache_operation
.size xm1_power_whole_cache,.-xm1_power_whole_cache
.global xm1_power_dcache_clean_invalidate_all
.type xm1_power_dcache_clean_invalidate_all,%function
.set xm1_power_dcache_clean_invalidate_all,xm1_power_whole_cache

.Lwhole_cache_operation:
    push {r4-r11,lr}
    mov r6,r2
    mrc p15,1,r0,c0,c0,1
    ands r3,r0,#0x07000000
    lsr r3,r3,#23
    beq 6f
    mov r10,#0
3:
    add r2,r10,r10,lsr #1
    lsr r1,r0,r2
    and r1,r1,#7
    cmp r1,#2
    blt 5f
    mcr p15,2,r10,c0,c0,0
    isb sy
    mrc p15,1,r1,c0,c0,0
    and r2,r1,#7
    add r2,r2,#4
    movw r4,#0x3ff
    ands r4,r4,r1,lsr #3
    clz r5,r4
    movw r7,#0x7fff
    ands r7,r7,r1,lsr #13
7:
    mov r9,r4
4:
    orr r11,r10,r9,lsl r5
    orr r11,r11,r7,lsl r2
    cmp r6,#1
    mcreq p15,0,r11,c7,c14,2
    cmp r6,#2
    mcreq p15,0,r11,c7,c10,2
    subs r9,r9,#1
    bge 4b
    subs r7,r7,#1
    bge 7b
5:
    add r10,r10,#2
    cmp r3,r10
    bgt 3b
6:
    pop {r4-r11,pc}

.global xm1_power_dcache_clean_range
.type xm1_power_dcache_clean_range,%function
xm1_power_dcache_clean_range:
    cmp r0,#0
    beq .Lclean_all
    cmp r1,#0
    beq .Lclean_all
    add r1,r0,r1
    bic r0,r0,#31
.Lclean_line:
    mcr p15,0,r0,c7,c10,1
    add r0,r0,#32
    cmp r0,r1
    blt .Lclean_line
    bx lr
.Lclean_all:
    mov r2,#2
    b .Lwhole_cache_operation
.size xm1_power_dcache_clean_range,.-xm1_power_dcache_clean_range
