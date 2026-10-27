// af-recorder.S - same-boot syslog-store carry for the X-M1 1.50 ordinary boot.
//
// Candidate payload, NOT installable. The blob is placed in the plain window's
// tail padding at RAM 0x0144a5e0 (flash media offset 0x8ea5e0), the id-6
// loader's raw-copy window [0x01100000, 0x01460000); one changed `bl` word in
// image 4 routes the id-6 completion tail through rec_stage after the copy.
// The blob claims [0x0144a5e0, 0x0144a5e0 + 0x800) and touches no byte of the
// D3 prefix [0x0061c1fc, 0x0061c400) or of the recipe bank's gap blocks
// [0x011ac5a0, 0x011ad714] and [0x011dcca0, 0x011dfe10]. The region is
// loaded-but-unowned zero padding on the window census (reports/recipe-
// design.md section 18); stock code executes from the same window, and the
// recorder's runtime state is written inside its own region.
//
// Residency. The receive site remains stock until the id-6 loader has returned.
// Its case-3 completion call is retargeted to rec_stage, which lives in the
// same 128 KiB copy block as rec_hook. rec_stage publishes the hook word,
// performs data/instruction cache maintenance for that word and tail-calls the
// displaced completion helper. A failed id-6 copy can still reach the
// unstaged trampoline; that media-failure residual is named in the manifest.
//
// Behaviour. On each media-loop invocation the hook examines at most SCAN_MAX
// store slots after its retained cursor and stops at the first trigger; when
// caught up it reads one header halfword. An attempt is opened by a type-0x0f
// record whose id is 0x3b0d or 0x3f2e (the AF request and the search start);
// 0x3f2e is remembered in the search flag. A result is the exact word
// 0x13a52b03 (receiver 3, sender 0x2b, id 0x13a5, the AF success result); the
// type byte is checked before the word. A second 0x3b0d with an attempt open
// closes the previous attempt with no recorded result; an attempt open past
// DEADLINE ticks closes with a timeout only after a caught-up scan, so a
// pending result record is always examined first; each fires the carry with
// its own trigger class. A writer wrap is counted once, when the next index
// falls below the last observed index, before any trigger can exit; when the
// next index also sits below the cursor the scan drains to the store end and
// resumes at slot 0. The trailer carries the wrap count and the next index at
// trigger, so an overflowing run is labelled rather than silently truncated.
// All classes fire one carry per boot.
//
// Carry. The hook runs the stock open/write/close chain once and writes the
// whole store (0x400a0800, 0x1fff4 bytes) as a live, non-atomic capture; when
// the store write completes it appends the recorder trailer, 40 bytes read
// from rec_trailer, whose status byte is refreshed with the first store-write
// error before the trailer is posted. The file is therefore store bytes
// followed by a trailer when its length is 0x1fff4 + 40; any other length is a
// short capture. Close status is not carried: it postdates the trailer. No
// allocation and no snapshot are needed, and every file call runs in the media
// task's context. No AF-path code is entered.
//
// Attempt identity and result coverage. One carry exists per boot; the trailer
// carries the trigger class (1 result, 2 attempt closed without a recorded
// result, 3 attempt deadline), the attempt ordinal, the retained attempt tick
// and the trigger tick, the seen counts and the wrap count, so a failed or
// overflowing run is distinguishable from a success without a second file.
// Statistic, AF-frame and lens freshness are state, not messages, and are not
// carried: the store dates the attempt and result messages at the tick
// resolution (10.01 ms), nothing more. The per-job prerequisite and
// terminal-outcome map is in the manifest's service_job_map block.
//
// Open entry point: 0x71c648 (callback, path, mode), the stock card-file
// writer's entry (WT_LOG 0x00ed1850, the auto-act loader 0x00ed1de0 and the
// S-record loader all call it). Its fifth argument is 1 and becomes the
// file-system open's own fourth argument (0x71f2a8 passes payload+0x4c to
// 0x701634), which is the observed value for creating a new card file. The
// fourth argument must be zero: it selects the client-side bookkeeping
// triple, and the guard at 0x71c2d0-0x71c2f8 refuses a nonzero fourth
// argument when the current task is 0x1c or 0, so only the callback-form
// twins are usable from here.
//
// File ownership. A nonzero open tag is retained through the close callback.
// Close is posted after the store, trailer or a write failure. The close worker
// 0x0071f1d4 releases the tag, removes its mapping and closes the file.
// A synchronous readiness rejection bypasses that worker, as does the outer
// close handler 0x00711768 when the volume pointer 0x00910204 is zero.
// The inner open worker 0x0071f2a8 releases a failed open's tag. The outer open
// handler 0x007115a4 can reject a queued open without entering that worker;
// its callback returns the initially zero completion tag, not the allocation.
// The client 0x0071c2b0 also queues opens after zero-tag allocation. A successful
// zero-tag callback sets NOHANDLE and posts neither a write nor a close.
// Queue failure is not delivered through these callbacks. The allocator
// 0x0071ef18 and release routine 0x0071eff4 wait on semaphore 0x3c with
// 0xffffffff. Callback completion does not establish ownership cleanup or a
// bounded wait.
//
// Error contract: the stock worker 0x0071ed34 forwards the completed count
// even on a nonzero status (0x006ff000 preserves the count from 0x007069b0
// when a later cluster step fails, and a full count when the commit 0x00703f28
// fails), so both the success and failure paths subtract the bounded count
// from the remaining word before anything else; the first error is kept in a
// status byte and every failure still posts the close. There is no retry.
//
// Cost. Caught up: one halfword load per invocation. Otherwise at most
// SCAN_MAX slot reads with a type byte, one word and up to two id compares
// each; the deadline check adds one tick load while an attempt is open. The
// scan and the deadline advance only when the media loop runs; a boot in which
// the media task never receives after the AF event carries nothing.
//
// Out-of-blob branches resolve through the absolute symbols af-recorder.ld
// defines (rec_open_wrap, rec_write_wrap, rec_close_wrap, rec_recv_impl,
// rec_stage_impl), so no numeric branch operand and no pinned site constant
// survives in this source. The linker script pins the blob base, rec_stage at
// base+0x3c0, the state block at base+0x700 and the trailer at base+0x740.
//
// Register rule: the media loop keeps r4-r7 and r11 live across the receive
// call, so this blob touches only r0-r3, r12 and lr.

.syntax unified
.arch armv7-r
.arm
.section .text,"ax",%progbits

.equ STORE,        0x400a0800
.equ STORE_NEXT,   0x1e          // halfword: next slot index
.equ STORE_SLOT0,  0x34          // first slot offset
.equ STORE_SLOTS,  8188          // 16-byte slots
.equ STORE_LEN,    0x1fff4       // whole store
.equ TICK_CELL,    0x400c0800    // 32-bit body tick, 10.01 ms per unit
.equ SCAN_MAX,     16            // slots examined per invocation
.equ DEADLINE,     1500          // ticks open an attempt may stay (about 15 s)
.equ RESULT_SIG,   0x13a52b03    // receiver 3, sender 0x2b, id 0x13a5
.equ ATTEMPT_REQ,  0x3b0d        // AF request id
.equ ATTEMPT_SRH,  0x3f2e        // search start id
.equ TRIG_RESULT,  1
.equ TRIG_NORESULT, 2
.equ TRIG_TIMEOUT, 3
.equ PHASE_STORE,  1
.equ PHASE_TRAILER, 2
.equ TRAILER_LEN,  40
.equ TRAILER_MAGIC, 0x43524641  // "AFRC"

// rec_state layout, pinned at REC_BASE + 0x700
.equ ST_FIRED,     0             // u8: carry started
.equ ST_STATUS,    1             // u8: first file error
.equ ST_PHASE,     2             // u8: 1 store write, 2 trailer write
.equ ST_OPEN,      3             // u8: attempt open
.equ ST_HANDLE,    4             // u32: handle tag
.equ ST_SOURCE,    8             // u32: write source
.equ ST_REMAIN,    12            // u32: write remaining
.equ ST_CURSOR,    16            // u16: next slot to examine
.equ ST_ATTEMPTS,  18            // u16: attempts opened
.equ ST_RESULTS,   20            // u16: result records seen
.equ ST_NORESULT,  22            // u16: attempts closed without a result
.equ ST_FLAGS,     24            // u32: F_* bits
.equ ST_WRAPS,     32            // u32: observed writer crossings
.equ ST_ATICK,     36            // u32: tick of the open attempt
.equ ST_TTICK,     40            // u32: tick at trigger
.equ ST_TCLASS,    44            // u8: trigger class
.equ ST_TATTEMPT,  46            // u16: attempt ordinal at trigger
.equ ST_TNEXT,     48            // u32: store next index at trigger
.equ ST_PREVNEXT,  52            // u16: last observed store next index

.equ F_WRAP,       1
.equ F_SEARCH,     4
.equ F_NOHANDLE,   8

.global rec_hook
.type rec_hook,%function
rec_hook:
    push    {r0, lr}                 // [sp] = caller's receive buffer
    movw    r0, #:lower16:rec_state
    movt    r0, #:upper16:rec_state
    ldrb    r3, [r0, #ST_FIRED]
    cmp     r3, #0
    bne     rec_recv                 // carry already started

    ldr     r2, =STORE
    ldrh    r1, [r0, #ST_CURSOR]
    ldrh    r12, [r2, #STORE_NEXT]
    ldrh    r3, [r0, #ST_PREVNEXT]
    strh    r12, [r0, #ST_PREVNEXT]
    cmp     r12, r3
    bhs     rec_scan_target         // next did not move backwards
    ldr     r3, [r0, #ST_FLAGS]      // the writer's index moved backwards: one wrap
    orr     r3, r3, #F_WRAP
    str     r3, [r0, #ST_FLAGS]
    ldr     r3, [r0, #ST_WRAPS]
    add     r3, r3, #1
    str     r3, [r0, #ST_WRAPS]
rec_scan_target:
    cmp     r12, r1
    bne     rec_scan_go
    b       rec_deadline             // caught up
rec_scan_go:
    bhi     rec_scan_window
    movw    r12, #STORE_SLOTS        // the reader is behind a wrap: drain to the store end
rec_scan_window:
    sub     r3, r12, r1
    cmp     r3, #SCAN_MAX
    movhi   r3, #SCAN_MAX
    lsl     lr, r1, #4
    add     r1, r2, #STORE_SLOT0
    add     r1, r1, lr

// r3 is the remaining slot budget for this invocation; the attempt and search
// handlers return here and keep their scratch in r12 only.
rec_scan:
    ldrb    lr, [r1]
    cmp     lr, #0x0f
    bne     rec_scan_next
    ldr     r12, [r1, #4]
    ldr     lr, =RESULT_SIG
    cmp     r12, lr
    beq     rec_result
    mov     lr, r12, lsr #16
    movw    r12, #ATTEMPT_REQ
    cmp     lr, r12
    beq     rec_attempt
    movw    r12, #ATTEMPT_SRH
    cmp     lr, r12
    beq     rec_search
rec_scan_next:
    add     r1, r1, #16
    subs    r3, r3, #1
    bne     rec_scan

    sub     lr, r1, r2               // r2 = STORE, untouched by the loop
    sub     lr, lr, #STORE_SLOT0
    lsr     lr, lr, #4               // advanced cursor
    movw    r12, #STORE_SLOTS
    cmp     lr, r12
    blo     rec_scan_store
    mov     lr, #0                   // store end: resume at the writer's wrap point
rec_scan_store:
    strh    lr, [r0, #ST_CURSOR]
    b       rec_recv                 // records were pending; a timeout waits for a clean pass

// an open attempt past the deadline fires only when the scan caught nothing:
// pending records, the success result among them, are examined first
rec_deadline:
    ldrb    r3, [r0, #ST_OPEN]
    cmp     r3, #0
    beq     rec_recv
    ldr     r3, =TICK_CELL
    ldr     r3, [r3]
    ldr     r12, [r0, #ST_ATICK]
    sub     r3, r3, r12
    movw    r12, #DEADLINE
    cmp     r3, r12
    blo     rec_recv
    ldrh    r3, [r0, #ST_NORESULT]
    add     r3, r3, #1
    strh    r3, [r0, #ST_NORESULT]
    mov     r3, #TRIG_TIMEOUT
    b       rec_fire

rec_result:
    ldrh    r3, [r0, #ST_RESULTS]
    add     r3, r3, #1
    strh    r3, [r0, #ST_RESULTS]
    mov     r3, #0
    strb    r3, [r0, #ST_OPEN]
    mov     r3, #TRIG_RESULT
    b       rec_fire

rec_attempt:
    ldrb    r12, [r0, #ST_OPEN]
    cmp     r12, #0
    beq     rec_attempt_open
    ldrh    r12, [r0, #ST_NORESULT]
    add     r12, r12, #1
    strh    r12, [r0, #ST_NORESULT]
    mov     r3, #TRIG_NORESULT       // firing path: r3 (the slot budget) is free
    b       rec_fire
rec_attempt_open:
    mov     r12, #1
    strb    r12, [r0, #ST_OPEN]
    ldrh    r12, [r0, #ST_ATTEMPTS]
    add     r12, r12, #1
    strh    r12, [r0, #ST_ATTEMPTS]
    ldr     r12, [r1, #0xc]          // record tick
    str     r12, [r0, #ST_ATICK]
    b       rec_scan_next

rec_search:
    ldrb    r12, [r0, #ST_OPEN]
    cmp     r12, #0
    bne     rec_search_flag
    mov     r12, #1
    strb    r12, [r0, #ST_OPEN]
    ldrh    r12, [r0, #ST_ATTEMPTS]
    add     r12, r12, #1
    strh    r12, [r0, #ST_ATTEMPTS]
    ldr     r12, [r1, #0xc]
    str     r12, [r0, #ST_ATICK]
rec_search_flag:
    ldr     r12, [r0, #ST_FLAGS]
    orr     r12, r12, #F_SEARCH
    str     r12, [r0, #ST_FLAGS]
    b       rec_scan_next

// trigger: latch the outcome, build the trailer, start the carry
rec_fire:
    strb    r3, [r0, #ST_TCLASS]
    mov     r3, #1
    strb    r3, [r0, #ST_FIRED]
    ldr     r3, =TICK_CELL
    ldr     r3, [r3]
    str     r3, [r0, #ST_TTICK]
    ldr     r2, =STORE
    ldrh    r3, [r2, #STORE_NEXT]
    str     r3, [r0, #ST_TNEXT]
    ldrh    r3, [r0, #ST_ATTEMPTS]
    strh    r3, [r0, #ST_TATTEMPT]
    movw    r1, #:lower16:rec_trailer
    movt    r1, #:upper16:rec_trailer
    ldr     r2, =TRAILER_MAGIC
    str     r2, [r1]
    mov     r2, #2
    strh    r2, [r1, #4]             // version 2: attempt tick appended
    ldrb    r2, [r0, #ST_TCLASS]
    strb    r2, [r1, #6]
    ldrb    r2, [r0, #ST_STATUS]
    strb    r2, [r1, #7]
    ldrh    r2, [r0, #ST_ATTEMPTS]
    strh    r2, [r1, #8]
    ldrh    r2, [r0, #ST_RESULTS]
    strh    r2, [r1, #10]
    ldrh    r2, [r0, #ST_NORESULT]
    strh    r2, [r1, #12]
    ldrh    r2, [r0, #ST_FLAGS]
    strh    r2, [r1, #14]
    ldr     r2, [r0, #ST_TTICK]
    str     r2, [r1, #16]
    ldrh    r2, [r0, #ST_TATTEMPT]
    strh    r2, [r1, #20]
    ldr     r2, [r0, #ST_WRAPS]
    str     r2, [r1, #24]
    ldr     r2, [r0, #ST_TNEXT]
    str     r2, [r1, #28]
    ldr     r2, =STORE_LEN
    str     r2, [r1, #32]
    ldr     r2, [r0, #ST_ATICK]      // retained attempt tick
    str     r2, [r1, #36]
    adr     r0, cb_open
    adr     r1, rec_path
    mov     r2, #1                   // mode 1 = 'w'
    bl      rec_open_wrap
    b       rec_recv

rec_recv:
    pop     {r0, lr}                 // restore the receive buffer pointer
    b       rec_recv_impl

// open completion: r0 = status, r1 = handle tag
cb_open:
    movw    r2, #:lower16:rec_state
    movt    r2, #:upper16:rec_state
    cmp     r0, #0
    bne     rec_fin
    cmp     r1, #0
    beq     rec_nohandle             // success with no tag: nothing addressable
    str     r1, [r2, #ST_HANDLE]
    ldr     r3, =STORE
    str     r3, [r2, #ST_SOURCE]
    mov     r3, #PHASE_STORE
    strb    r3, [r2, #ST_PHASE]
    ldr     r3, =STORE_LEN
    str     r3, [r2, #ST_REMAIN]
    adr     r0, cb_write
    ldr     r1, [r2, #ST_HANDLE]
    ldr     r2, [r2, #ST_REMAIN]
    ldr     r3, =STORE
    b       rec_write_wrap

rec_nohandle:
    ldr     r3, [r2, #ST_FLAGS]
    orr     r3, r3, #F_NOHANDLE
    str     r3, [r2, #ST_FLAGS]
    bx      lr

// write completion: r0 = status, r1 = bytes completed by this call
cb_write:
    movw    r12, #:lower16:rec_state
    movt    r12, #:upper16:rec_state
    ldr     r2, [r12, #ST_REMAIN]
    cmp     r0, #0
    bne     rec_wfail
    subs    r2, r2, r1
    str     r2, [r12, #ST_REMAIN]
    ldr     r3, [r12, #ST_SOURCE]
    add     r3, r3, r1
    str     r3, [r12, #ST_SOURCE]
    cmp     r2, #0
    ble     rec_phase_done
    ldr     r1, [r12, #ST_HANDLE]
    adr     r0, cb_write
    b       rec_write_wrap           // r2 = length, r3 = source, per the ABI

// write failure: account the bounded completed count, keep the error, close
rec_wfail:
    cmp     r1, r2                   // count <= remaining
    movhi   r1, r2
    subs    r2, r2, r1
    str     r2, [r12, #ST_REMAIN]
    ldrb    r3, [r12, #ST_STATUS]
    cmp     r3, #0
    strbeq  r0, [r12, #ST_STATUS]    // first error wins
    b       rec_close

// store write complete: append the trailer, or close when it is written
rec_phase_done:
    ldrb    r3, [r12, #ST_PHASE]
    cmp     r3, #PHASE_STORE
    bne     rec_close
    mov     r3, #PHASE_TRAILER
    strb    r3, [r12, #ST_PHASE]
    movw    r3, #:lower16:rec_trailer
    movt    r3, #:upper16:rec_trailer
    ldrb    r2, [r12, #ST_STATUS]    // first store-write error, if any
    strb    r2, [r3, #7]
    str     r3, [r12, #ST_SOURCE]
    mov     r2, #TRAILER_LEN
    str     r2, [r12, #ST_REMAIN]
    adr     r0, cb_write
    ldr     r1, [r12, #ST_HANDLE]
    ldr     r3, [r12, #ST_SOURCE]
    b       rec_write_wrap

rec_close:
    ldrb    r2, [r12, #ST_STATUS]
    cmp     r2, #0
    strbeq  r0, [r12, #ST_STATUS]
    ldr     r1, [r12, #ST_HANDLE]
    mov     r2, #0
    mov     r3, #0
    adr     r0, cb_close
    b       rec_close_wrap

// close completion, open failure and zero-tag: r0 = status
cb_close:
rec_fin:
    movw    r1, #:lower16:rec_state
    movt    r1, #:upper16:rec_state
    ldrb    r2, [r1, #ST_STATUS]
    cmp     r2, #0
    strbeq  r0, [r1, #ST_STATUS]     // keep the first error
    bx      lr

.align 2
.balign 4
.ltorg
rec_path:
    .asciz "C:\\AF.BIN"
.balign 4

.size rec_hook, .-rec_hook

// Publish the receive hook only after the id-6 copy returns, then preserve the
// displaced completion call's arguments and link register through a tail call.
.section .stage,"ax",%progbits
.balign 4
.global rec_stage
.type rec_stage,%function
rec_stage:
    push    {r0-r3, r12, lr}
    movw    r0, #0xfeb4
    movt    r0, #0x0071
    movw    r1, #0xa9c9
    movt    r1, #0xeb34
    str     r1, [r0]
    mcr     p15, 0, r0, c7, c10, 1
    dsb     sy
    mcr     p15, 0, r0, c7, c5, 1
    dsb     sy
    isb     sy
    pop     {r0-r3, r12, lr}
    b       rec_stage_impl
.size rec_stage, .-rec_stage

// The working state, zero in the container so every boot starts clean.
.section .state,"aw",%progbits
.balign 4
.global rec_state
.type rec_state,%object
rec_state:
    .zero 64
.size rec_state, 64

// The trailer appended to the capture, version 2: magic, version, class,
// status (the first store-write error, refreshed before the write), counts,
// flags, trigger tick, attempt ordinal, wrap count, next index, the store
// length the reader splits the file at, and the retained attempt tick.
.section .trailer,"aw",%progbits
.balign 4
.global rec_trailer
.type rec_trailer,%object
rec_trailer:
    .zero TRAILER_LEN
.size rec_trailer, TRAILER_LEN
