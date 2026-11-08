.syntax unified
.arm

.section .text.fp_entry_create,"ax",%progbits
.global xm1_rtos_fixed_pool_create
.type xm1_rtos_fixed_pool_create,%function
xm1_rtos_fixed_pool_create:
    b xm1_rtos_fixed_pool_create_body
.size xm1_rtos_fixed_pool_create, .-xm1_rtos_fixed_pool_create

.section .text.fp_entry_delete,"ax",%progbits
.global xm1_rtos_fixed_pool_delete
.type xm1_rtos_fixed_pool_delete,%function
xm1_rtos_fixed_pool_delete:
    b xm1_rtos_fixed_pool_delete_body
.size xm1_rtos_fixed_pool_delete, .-xm1_rtos_fixed_pool_delete

.section .text.fp_entry_get,"ax",%progbits
.global xm1_rtos_fixed_pool_get
.type xm1_rtos_fixed_pool_get,%function
xm1_rtos_fixed_pool_get:
    b xm1_rtos_fixed_pool_get_body
.size xm1_rtos_fixed_pool_get, .-xm1_rtos_fixed_pool_get

.section .text.fp_entry_poll,"ax",%progbits
.global xm1_rtos_fixed_pool_poll
.type xm1_rtos_fixed_pool_poll,%function
xm1_rtos_fixed_pool_poll:
    b xm1_rtos_fixed_pool_poll_body
.size xm1_rtos_fixed_pool_poll, .-xm1_rtos_fixed_pool_poll

.section .text.fp_entry_timed,"ax",%progbits
.global xm1_rtos_fixed_pool_timed_get
.type xm1_rtos_fixed_pool_timed_get,%function
xm1_rtos_fixed_pool_timed_get:
    b xm1_rtos_fixed_pool_timed_get_body
.size xm1_rtos_fixed_pool_timed_get, .-xm1_rtos_fixed_pool_timed_get

.section .text.fp_entry_release,"ax",%progbits
.global xm1_rtos_fixed_pool_release
.type xm1_rtos_fixed_pool_release,%function
xm1_rtos_fixed_pool_release:
    b xm1_rtos_fixed_pool_release_body
.size xm1_rtos_fixed_pool_release, .-xm1_rtos_fixed_pool_release

.section .text.fp_entry_unchecked,"ax",%progbits
.global xm1_rtos_fixed_pool_poll_unchecked
.type xm1_rtos_fixed_pool_poll_unchecked,%function
xm1_rtos_fixed_pool_poll_unchecked:
    b xm1_rtos_fixed_pool_poll_unchecked_body
.size xm1_rtos_fixed_pool_poll_unchecked, .-xm1_rtos_fixed_pool_poll_unchecked
