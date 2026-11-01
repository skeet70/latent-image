.syntax unified
.arch armv7-r
.arm

.section .text.heap_replay_stock_lock,"ax",%progbits
.global heap_replay_stock_lock
.type heap_replay_stock_lock,%function
heap_replay_stock_lock:
    ldr ip, =0x0072b164
    bx ip
.size heap_replay_stock_lock, .-heap_replay_stock_lock
.ltorg

.section .text.heap_replay_stock_unlock,"ax",%progbits
.global heap_replay_stock_unlock
.type heap_replay_stock_unlock,%function
heap_replay_stock_unlock:
    ldr ip, =0x0072b1cc
    bx ip
.size heap_replay_stock_unlock, .-heap_replay_stock_unlock
.ltorg

.section .text.heap_replay_panic,"ax",%progbits
.global heap_replay_panic
.type heap_replay_panic,%function
heap_replay_panic:
    udf #0
    b heap_replay_panic
.size heap_replay_panic, .-heap_replay_panic

.section .note.GNU-stack,"",%progbits
