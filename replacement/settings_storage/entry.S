    .syntax unified
    .arm
    .section .text.memcpy,"ax",%progbits
    .balign 4
    .global memcpy
    .type memcpy,%function
memcpy:
    mov r3, r0
    cmp r2, #1024
    bls 2f
    eor r12, r0, r1
    tst r12, #3
    bne 2f
1:  ldrb r12, [r1], #1
    strb r12, [r3], #1
    sub r2, r2, #1
    tst r3, #3
    bne 1b
1:  cmp r2, #4
    blo 3f
    ldr r12, [r1], #4
    str r12, [r3], #4
    sub r2, r2, #4
    b 1b
2:  orr r12, r0, r1
    tst r12, #1
    bne 3f
1:  cmp r2, #2
    blo 3f
    ldrh r12, [r1], #2
    strh r12, [r3], #2
    sub r2, r2, #2
    b 1b
3:  cmp r2, #0
    bxeq lr
    ldrb r12, [r1], #1
    strb r12, [r3], #1
    sub r2, r2, #1
    b 3b
    .size memcpy, . - memcpy
    .global __aeabi_memcpy
    .set __aeabi_memcpy, memcpy
    .global __aeabi_memcpy4
    .set __aeabi_memcpy4, memcpy
    .section .text.memset,"ax",%progbits
    .balign 4
    .global memset
    .type memset,%function
memset:
    mov r3, r0
1:  cmp r2, #0
    bxeq lr
    strb r1, [r3], #1
    sub r2, r2, #1
    b 1b
    .size memset, . - memset
    .section .text.__aeabi_memset,"ax",%progbits
    .balign 4
    .global __aeabi_memset
    .type __aeabi_memset,%function
__aeabi_memset:
    mov r3, r1
    mov r1, r2
    mov r2, r3
    b memset
    .size __aeabi_memset, . - __aeabi_memset
    .section .text.memmove,"ax",%progbits
    .balign 4
    .global memmove
    .type memmove,%function
memmove:
    cmp r0, r1
    bls memcpy
    sub r3, r0, r1
    cmp r3, r2
    bhs memcpy
    add r3, r0, r2
    add r1, r1, r2
1:  cmp r2, #0
    bxeq lr
    ldrb r12, [r1, #-1]!
    strb r12, [r3, #-1]!
    sub r2, r2, #1
    b 1b
    .size memmove, . - memmove
    .global __aeabi_memmove
    .set __aeabi_memmove, memmove
