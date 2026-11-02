.syntax unified
.arm

@ Public wrappers enter through the core entry address, as the stock wrappers call it.
.macro public_entry name, operation, arity
.section .heap.entry.\name,"ax",%progbits
.global xm1_heap_\name\()_entry
.type xm1_heap_\name\()_entry,%function
xm1_heap_\name\()_entry:
.if \arity == 2
    mov r2, r1
.endif
    mov r1, r0
    movw r0, #0xc970
    movt r0, #0x89
    ldr r0, [r0]
    b xm1_heap_\operation\()_r_entry
.size xm1_heap_\name\()_entry, .-xm1_heap_\name\()_entry
.endm

public_entry malloc, malloc, 1
public_entry free, free, 1
public_entry realloc, realloc, 2
public_entry calloc, calloc, 2

.macro core_entry name
.section .heap.entry.\name\()_r,"ax",%progbits
.global xm1_heap_\name\()_r_entry
.type xm1_heap_\name\()_r_entry,%function
xm1_heap_\name\()_r_entry:
    b xm1_heap_\name\()_r
.size xm1_heap_\name\()_r_entry, .-xm1_heap_\name\()_r_entry
.endm
core_entry malloc
core_entry free
core_entry realloc
core_entry calloc
core_entry trim

.section .heap.support,"ax",%progbits
.global xm1_heap_lock
.type xm1_heap_lock,%function
xm1_heap_lock:
    b __heap_retained_lock
.size xm1_heap_lock, .-xm1_heap_lock
.global xm1_heap_unlock
.type xm1_heap_unlock,%function
xm1_heap_unlock:
    b __heap_retained_unlock
.size xm1_heap_unlock, .-xm1_heap_unlock
.global xm1_heap_panic
.type xm1_heap_panic,%function
xm1_heap_panic:
    b xm1_heap_panic
.size xm1_heap_panic, .-xm1_heap_panic

.section .heap.sentinel,"aw",%progbits
.global __heap_sentinel
.type __heap_sentinel,%object
__heap_sentinel:
    .word 0
.size __heap_sentinel, 4
