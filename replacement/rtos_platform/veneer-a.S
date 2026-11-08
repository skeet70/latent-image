/* Derived from carriers.json; regenerate when the union changes. */
.syntax unified
.arm

.section .veneer.s1_acquire,"ax",%progbits
.global xm1_platform_veneer_s1_acquire
.type xm1_platform_veneer_s1_acquire,%function
xm1_platform_veneer_s1_acquire:
    b xm1_rtos_scheduler_acquire
.size xm1_platform_veneer_s1_acquire, .-xm1_platform_veneer_s1_acquire

.section .veneer.s1_release,"ax",%progbits
.global xm1_platform_veneer_s1_release
.type xm1_platform_veneer_s1_release,%function
xm1_platform_veneer_s1_release:
    b xm1_rtos_scheduler_release
.size xm1_platform_veneer_s1_release, .-xm1_platform_veneer_s1_release

.section .veneer.s1_find,"ax",%progbits
.global xm1_platform_veneer_s1_find
.type xm1_platform_veneer_s1_find,%function
xm1_platform_veneer_s1_find:
    b xm1_rtos_scheduler_find
.size xm1_platform_veneer_s1_find, .-xm1_platform_veneer_s1_find

.section .veneer.s1_insert,"ax",%progbits
.global xm1_platform_veneer_s1_insert
.type xm1_platform_veneer_s1_insert,%function
xm1_platform_veneer_s1_insert:
    b xm1_rtos_scheduler_insert
.size xm1_platform_veneer_s1_insert, .-xm1_platform_veneer_s1_insert

.section .veneer.s1_remove,"ax",%progbits
.global xm1_platform_veneer_s1_remove
.type xm1_platform_veneer_s1_remove,%function
xm1_platform_veneer_s1_remove:
    b xm1_rtos_scheduler_remove
.size xm1_platform_veneer_s1_remove, .-xm1_platform_veneer_s1_remove

.section .veneer.s1_reset,"ax",%progbits
.global xm1_platform_veneer_s1_reset
.type xm1_platform_veneer_s1_reset,%function
xm1_platform_veneer_s1_reset:
    b xm1_rtos_scheduler_reset
.size xm1_platform_veneer_s1_reset, .-xm1_platform_veneer_s1_reset

.section .veneer.s1_prepare,"ax",%progbits
.global xm1_platform_veneer_s1_prepare
.type xm1_platform_veneer_s1_prepare,%function
xm1_platform_veneer_s1_prepare:
    b xm1_rtos_scheduler_prepare
.size xm1_platform_veneer_s1_prepare, .-xm1_platform_veneer_s1_prepare

.section .veneer.s1_notify,"ax",%progbits
.global xm1_platform_veneer_s1_notify
.type xm1_platform_veneer_s1_notify,%function
xm1_platform_veneer_s1_notify:
    b xm1_rtos_scheduler_notify
.size xm1_platform_veneer_s1_notify, .-xm1_platform_veneer_s1_notify

.section .veneer.s1_requeue,"ax",%progbits
.global xm1_platform_veneer_s1_requeue
.type xm1_platform_veneer_s1_requeue,%function
xm1_platform_veneer_s1_requeue:
    b xm1_rtos_scheduler_requeue
.size xm1_platform_veneer_s1_requeue, .-xm1_platform_veneer_s1_requeue

.section .veneer.s1_rotate,"ax",%progbits
.global xm1_platform_veneer_s1_rotate
.type xm1_platform_veneer_s1_rotate,%function
xm1_platform_veneer_s1_rotate:
    b xm1_rtos_scheduler_rotate
.size xm1_platform_veneer_s1_rotate, .-xm1_platform_veneer_s1_rotate

.section .veneer.s2_irq_doorbell,"ax",%progbits
.global xm1_platform_veneer_s2_irq_doorbell
.type xm1_platform_veneer_s2_irq_doorbell,%function
xm1_platform_veneer_s2_irq_doorbell:
    b xm1_rtos_scheduler_doorbell
.size xm1_platform_veneer_s2_irq_doorbell, .-xm1_platform_veneer_s2_irq_doorbell

.section .veneer.s2_dispatch_save,"ax",%progbits
.global xm1_platform_veneer_s2_dispatch_save
.type xm1_platform_veneer_s2_dispatch_save,%function
xm1_platform_veneer_s2_dispatch_save:
    b xm1_rtos_scheduler_save
.size xm1_platform_veneer_s2_dispatch_save, .-xm1_platform_veneer_s2_dispatch_save

.section .veneer.s2_dispatch_resume,"ax",%progbits
.global xm1_platform_veneer_s2_dispatch_resume
.type xm1_platform_veneer_s2_dispatch_resume,%function
xm1_platform_veneer_s2_dispatch_resume:
    b xm1_rtos_scheduler_resume
.size xm1_platform_veneer_s2_dispatch_resume, .-xm1_platform_veneer_s2_dispatch_resume

.section .veneer.s2_dispatch_no_save,"ax",%progbits
.global xm1_platform_veneer_s2_dispatch_no_save
.type xm1_platform_veneer_s2_dispatch_no_save,%function
xm1_platform_veneer_s2_dispatch_no_save:
    b xm1_rtos_scheduler_no_save
.size xm1_platform_veneer_s2_dispatch_no_save, .-xm1_platform_veneer_s2_dispatch_no_save

.section .veneer.s2_dispatch_select,"ax",%progbits
.global xm1_platform_veneer_s2_dispatch_select
.type xm1_platform_veneer_s2_dispatch_select,%function
xm1_platform_veneer_s2_dispatch_select:
    b xm1_rtos_scheduler_select
.size xm1_platform_veneer_s2_dispatch_select, .-xm1_platform_veneer_s2_dispatch_select

.section .veneer.s2_dispatch_first_entry,"ax",%progbits
.global xm1_platform_veneer_s2_dispatch_first_entry
.type xm1_platform_veneer_s2_dispatch_first_entry,%function
xm1_platform_veneer_s2_dispatch_first_entry:
    b xm1_rtos_scheduler_first_entry
.size xm1_platform_veneer_s2_dispatch_first_entry, .-xm1_platform_veneer_s2_dispatch_first_entry

.section .veneer.s2_dispatch_irq_return,"ax",%progbits
.global xm1_platform_veneer_s2_dispatch_irq_return
.type xm1_platform_veneer_s2_dispatch_irq_return,%function
xm1_platform_veneer_s2_dispatch_irq_return:
    b xm1_rtos_scheduler_irq_return
.size xm1_platform_veneer_s2_dispatch_irq_return, .-xm1_platform_veneer_s2_dispatch_irq_return

.section .veneer.s2_dispatch_irq_resume,"ax",%progbits
.global xm1_platform_veneer_s2_dispatch_irq_resume
.type xm1_platform_veneer_s2_dispatch_irq_resume,%function
xm1_platform_veneer_s2_dispatch_irq_resume:
    b xm1_rtos_scheduler_irq_resume
.size xm1_platform_veneer_s2_dispatch_irq_resume, .-xm1_platform_veneer_s2_dispatch_irq_resume

.section .veneer.s2_dispatch_irq_tail,"ax",%progbits
.global xm1_platform_veneer_s2_dispatch_irq_tail
.type xm1_platform_veneer_s2_dispatch_irq_tail,%function
xm1_platform_veneer_s2_dispatch_irq_tail:
    b xm1_rtos_scheduler_irq_tail
.size xm1_platform_veneer_s2_dispatch_irq_tail, .-xm1_platform_veneer_s2_dispatch_irq_tail

.section .veneer.s3a_xm1_s3a_exit_task,"ax",%progbits
.global xm1_platform_veneer_s3a_xm1_s3a_exit_task
.type xm1_platform_veneer_s3a_xm1_s3a_exit_task,%function
xm1_platform_veneer_s3a_xm1_s3a_exit_task:
    b xm1_s3a_exit_task
.size xm1_platform_veneer_s3a_xm1_s3a_exit_task, .-xm1_platform_veneer_s3a_xm1_s3a_exit_task

.section .veneer.s3a_xm1_s3a_rotate_ready,"ax",%progbits
.global xm1_platform_veneer_s3a_xm1_s3a_rotate_ready
.type xm1_platform_veneer_s3a_xm1_s3a_rotate_ready,%function
xm1_platform_veneer_s3a_xm1_s3a_rotate_ready:
    b xm1_s3a_rotate_ready
.size xm1_platform_veneer_s3a_xm1_s3a_rotate_ready, .-xm1_platform_veneer_s3a_xm1_s3a_rotate_ready

.section .veneer.s3a_xm1_s3a_interrupt_rotate_ready,"ax",%progbits
.global xm1_platform_veneer_s3a_xm1_s3a_interrupt_rotate_ready
.type xm1_platform_veneer_s3a_xm1_s3a_interrupt_rotate_ready,%function
xm1_platform_veneer_s3a_xm1_s3a_interrupt_rotate_ready:
    b xm1_s3a_interrupt_rotate_ready
.size xm1_platform_veneer_s3a_xm1_s3a_interrupt_rotate_ready, .-xm1_platform_veneer_s3a_xm1_s3a_interrupt_rotate_ready

.section .veneer.s3a_xm1_s3a_core_rotate_ready,"ax",%progbits
.global xm1_platform_veneer_s3a_xm1_s3a_core_rotate_ready
.type xm1_platform_veneer_s3a_xm1_s3a_core_rotate_ready,%function
xm1_platform_veneer_s3a_xm1_s3a_core_rotate_ready:
    b xm1_s3a_core_rotate_ready
.size xm1_platform_veneer_s3a_xm1_s3a_core_rotate_ready, .-xm1_platform_veneer_s3a_xm1_s3a_core_rotate_ready

.section .veneer.s3a_xm1_s3a_interrupt_core_rotate_ready,"ax",%progbits
.global xm1_platform_veneer_s3a_xm1_s3a_interrupt_core_rotate_ready
.type xm1_platform_veneer_s3a_xm1_s3a_interrupt_core_rotate_ready,%function
xm1_platform_veneer_s3a_xm1_s3a_interrupt_core_rotate_ready:
    b xm1_s3a_interrupt_core_rotate_ready
.size xm1_platform_veneer_s3a_xm1_s3a_interrupt_core_rotate_ready, .-xm1_platform_veneer_s3a_xm1_s3a_interrupt_core_rotate_ready

.section .veneer.s3a_xm1_s3a_disable_dispatch,"ax",%progbits
.global xm1_platform_veneer_s3a_xm1_s3a_disable_dispatch
.type xm1_platform_veneer_s3a_xm1_s3a_disable_dispatch,%function
xm1_platform_veneer_s3a_xm1_s3a_disable_dispatch:
    b xm1_s3a_disable_dispatch
.size xm1_platform_veneer_s3a_xm1_s3a_disable_dispatch, .-xm1_platform_veneer_s3a_xm1_s3a_disable_dispatch

.section .veneer.s3a_xm1_s3a_enable_dispatch,"ax",%progbits
.global xm1_platform_veneer_s3a_xm1_s3a_enable_dispatch
.type xm1_platform_veneer_s3a_xm1_s3a_enable_dispatch,%function
xm1_platform_veneer_s3a_xm1_s3a_enable_dispatch:
    b xm1_s3a_enable_dispatch
.size xm1_platform_veneer_s3a_xm1_s3a_enable_dispatch, .-xm1_platform_veneer_s3a_xm1_s3a_enable_dispatch

.section .veneer.s3b_xm1_s3b_activate,"ax",%progbits
.global xm1_platform_veneer_s3b_xm1_s3b_activate
.type xm1_platform_veneer_s3b_xm1_s3b_activate,%function
xm1_platform_veneer_s3b_xm1_s3b_activate:
    b xm1_s3b_activate
.size xm1_platform_veneer_s3b_xm1_s3b_activate, .-xm1_platform_veneer_s3b_xm1_s3b_activate

.section .veneer.s3b_xm1_s3b_interrupt_activate,"ax",%progbits
.global xm1_platform_veneer_s3b_xm1_s3b_interrupt_activate
.type xm1_platform_veneer_s3b_xm1_s3b_interrupt_activate,%function
xm1_platform_veneer_s3b_xm1_s3b_interrupt_activate:
    b xm1_s3b_interrupt_activate
.size xm1_platform_veneer_s3b_xm1_s3b_interrupt_activate, .-xm1_platform_veneer_s3b_xm1_s3b_interrupt_activate

.section .veneer.s3b_xm1_s3b_cancel_activation,"ax",%progbits
.global xm1_platform_veneer_s3b_xm1_s3b_cancel_activation
.type xm1_platform_veneer_s3b_xm1_s3b_cancel_activation,%function
xm1_platform_veneer_s3b_xm1_s3b_cancel_activation:
    b xm1_s3b_cancel_activation
.size xm1_platform_veneer_s3b_xm1_s3b_cancel_activation, .-xm1_platform_veneer_s3b_xm1_s3b_cancel_activation

.section .veneer.s3b_xm1_s3b_wake,"ax",%progbits
.global xm1_platform_veneer_s3b_xm1_s3b_wake
.type xm1_platform_veneer_s3b_xm1_s3b_wake,%function
xm1_platform_veneer_s3b_xm1_s3b_wake:
    b xm1_s3b_wake
.size xm1_platform_veneer_s3b_xm1_s3b_wake, .-xm1_platform_veneer_s3b_xm1_s3b_wake

.section .veneer.s3b_xm1_s3b_interrupt_wake,"ax",%progbits
.global xm1_platform_veneer_s3b_xm1_s3b_interrupt_wake
.type xm1_platform_veneer_s3b_xm1_s3b_interrupt_wake,%function
xm1_platform_veneer_s3b_xm1_s3b_interrupt_wake:
    b xm1_s3b_interrupt_wake
.size xm1_platform_veneer_s3b_xm1_s3b_interrupt_wake, .-xm1_platform_veneer_s3b_xm1_s3b_interrupt_wake

.section .veneer.s3b_xm1_s3b_cancel_wake,"ax",%progbits
.global xm1_platform_veneer_s3b_xm1_s3b_cancel_wake
.type xm1_platform_veneer_s3b_xm1_s3b_cancel_wake,%function
xm1_platform_veneer_s3b_xm1_s3b_cancel_wake:
    b xm1_s3b_cancel_wake
.size xm1_platform_veneer_s3b_xm1_s3b_cancel_wake, .-xm1_platform_veneer_s3b_xm1_s3b_cancel_wake

.section .veneer.s3c_s3c_exception_loop,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_exception_loop
.type xm1_platform_veneer_s3c_s3c_exception_loop,%function
xm1_platform_veneer_s3c_s3c_exception_loop:
    b s3c_exception_loop
.size xm1_platform_veneer_s3c_s3c_exception_loop, .-xm1_platform_veneer_s3c_s3c_exception_loop

.section .veneer.s3c_s3c_exception_delivery,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_exception_delivery
.type xm1_platform_veneer_s3c_s3c_exception_delivery,%function
xm1_platform_veneer_s3c_s3c_exception_delivery:
    b s3c_exception_delivery
.size xm1_platform_veneer_s3c_s3c_exception_delivery, .-xm1_platform_veneer_s3c_s3c_exception_delivery

.section .veneer.s3c_s3c_get_priority,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_get_priority
.type xm1_platform_veneer_s3c_s3c_get_priority,%function
xm1_platform_veneer_s3c_s3c_get_priority:
    b s3c_get_priority
.size xm1_platform_veneer_s3c_s3c_get_priority, .-xm1_platform_veneer_s3c_s3c_get_priority

.section .veneer.s3c_s3c_sleep,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_sleep
.type xm1_platform_veneer_s3c_s3c_sleep,%function
xm1_platform_veneer_s3c_s3c_sleep:
    b s3c_sleep
.size xm1_platform_veneer_s3c_s3c_sleep, .-xm1_platform_veneer_s3c_s3c_sleep

.section .veneer.s3c_s3c_suspend,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_suspend
.type xm1_platform_veneer_s3c_s3c_suspend,%function
xm1_platform_veneer_s3c_s3c_suspend:
    b s3c_suspend
.size xm1_platform_veneer_s3c_s3c_suspend, .-xm1_platform_veneer_s3c_s3c_suspend

.section .veneer.s3c_s3c_resume,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_resume
.type xm1_platform_veneer_s3c_s3c_resume,%function
xm1_platform_veneer_s3c_s3c_resume:
    b s3c_resume
.size xm1_platform_veneer_s3c_s3c_resume, .-xm1_platform_veneer_s3c_s3c_resume

.section .veneer.s3c_s3c_force_resume,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_force_resume
.type xm1_platform_veneer_s3c_s3c_force_resume,%function
xm1_platform_veneer_s3c_s3c_force_resume:
    b s3c_force_resume
.size xm1_platform_veneer_s3c_s3c_force_resume, .-xm1_platform_veneer_s3c_s3c_force_resume

.section .veneer.s3c_s3c_raise,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_raise
.type xm1_platform_veneer_s3c_s3c_raise,%function
xm1_platform_veneer_s3c_s3c_raise:
    b s3c_raise
.size xm1_platform_veneer_s3c_s3c_raise, .-xm1_platform_veneer_s3c_s3c_raise

.section .veneer.s3c_s3c_interrupt_raise,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_interrupt_raise
.type xm1_platform_veneer_s3c_s3c_interrupt_raise,%function
xm1_platform_veneer_s3c_s3c_interrupt_raise:
    b s3c_interrupt_raise
.size xm1_platform_veneer_s3c_s3c_interrupt_raise, .-xm1_platform_veneer_s3c_s3c_interrupt_raise

.section .veneer.s3c_s3c_disable_exception,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_disable_exception
.type xm1_platform_veneer_s3c_s3c_disable_exception,%function
xm1_platform_veneer_s3c_s3c_disable_exception:
    b s3c_disable_exception
.size xm1_platform_veneer_s3c_s3c_disable_exception, .-xm1_platform_veneer_s3c_s3c_disable_exception

.section .veneer.s3c_s3c_enable_exception,"ax",%progbits
.global xm1_platform_veneer_s3c_s3c_enable_exception
.type xm1_platform_veneer_s3c_s3c_enable_exception,%function
xm1_platform_veneer_s3c_s3c_enable_exception:
    b s3c_enable_exception
.size xm1_platform_veneer_s3c_s3c_enable_exception, .-xm1_platform_veneer_s3c_s3c_enable_exception

.section .veneer.own_first_slot_60,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_60
.type xm1_platform_veneer_own_first_slot_60,%function
xm1_platform_veneer_own_first_slot_60:
    b xm1_rtos_own_first_slot_60
.size xm1_platform_veneer_own_first_slot_60, .-xm1_platform_veneer_own_first_slot_60

.section .veneer.own_first_slot_fc,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_fc
.type xm1_platform_veneer_own_first_slot_fc,%function
xm1_platform_veneer_own_first_slot_fc:
    b xm1_rtos_own_first_slot_fc
.size xm1_platform_veneer_own_first_slot_fc, .-xm1_platform_veneer_own_first_slot_fc

.section .veneer.own_first_slot_100,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_100
.type xm1_platform_veneer_own_first_slot_100,%function
xm1_platform_veneer_own_first_slot_100:
    b xm1_rtos_own_first_slot_100
.size xm1_platform_veneer_own_first_slot_100, .-xm1_platform_veneer_own_first_slot_100

.section .veneer.own_first_slot_128,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_128
.type xm1_platform_veneer_own_first_slot_128,%function
xm1_platform_veneer_own_first_slot_128:
    b xm1_rtos_own_first_slot_128
.size xm1_platform_veneer_own_first_slot_128, .-xm1_platform_veneer_own_first_slot_128

.section .veneer.own_first_slot_12c,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_12c
.type xm1_platform_veneer_own_first_slot_12c,%function
xm1_platform_veneer_own_first_slot_12c:
    b xm1_rtos_own_first_slot_12c
.size xm1_platform_veneer_own_first_slot_12c, .-xm1_platform_veneer_own_first_slot_12c

.section .veneer.own_first_slot_130,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_130
.type xm1_platform_veneer_own_first_slot_130,%function
xm1_platform_veneer_own_first_slot_130:
    b xm1_rtos_own_first_slot_130
.size xm1_platform_veneer_own_first_slot_130, .-xm1_platform_veneer_own_first_slot_130

.section .veneer.own_first_slot_134,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_134
.type xm1_platform_veneer_own_first_slot_134,%function
xm1_platform_veneer_own_first_slot_134:
    b xm1_rtos_own_first_slot_134
.size xm1_platform_veneer_own_first_slot_134, .-xm1_platform_veneer_own_first_slot_134

.section .veneer.own_first_slot_138,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_138
.type xm1_platform_veneer_own_first_slot_138,%function
xm1_platform_veneer_own_first_slot_138:
    b xm1_rtos_own_first_slot_138
.size xm1_platform_veneer_own_first_slot_138, .-xm1_platform_veneer_own_first_slot_138

.section .veneer.own_first_slot_13c,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_13c
.type xm1_platform_veneer_own_first_slot_13c,%function
xm1_platform_veneer_own_first_slot_13c:
    b xm1_rtos_own_first_slot_13c
.size xm1_platform_veneer_own_first_slot_13c, .-xm1_platform_veneer_own_first_slot_13c

.section .veneer.own_first_slot_148,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_148
.type xm1_platform_veneer_own_first_slot_148,%function
xm1_platform_veneer_own_first_slot_148:
    b xm1_rtos_own_first_slot_148
.size xm1_platform_veneer_own_first_slot_148, .-xm1_platform_veneer_own_first_slot_148

.section .veneer.own_first_slot_14c,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_14c
.type xm1_platform_veneer_own_first_slot_14c,%function
xm1_platform_veneer_own_first_slot_14c:
    b xm1_rtos_own_first_slot_14c
.size xm1_platform_veneer_own_first_slot_14c, .-xm1_platform_veneer_own_first_slot_14c

.section .veneer.own_first_slot_150,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_150
.type xm1_platform_veneer_own_first_slot_150,%function
xm1_platform_veneer_own_first_slot_150:
    b xm1_rtos_own_first_slot_150
.size xm1_platform_veneer_own_first_slot_150, .-xm1_platform_veneer_own_first_slot_150

.section .veneer.own_first_slot_154,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_154
.type xm1_platform_veneer_own_first_slot_154,%function
xm1_platform_veneer_own_first_slot_154:
    b xm1_rtos_own_first_slot_154
.size xm1_platform_veneer_own_first_slot_154, .-xm1_platform_veneer_own_first_slot_154

.section .veneer.own_first_slot_164,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_164
.type xm1_platform_veneer_own_first_slot_164,%function
xm1_platform_veneer_own_first_slot_164:
    b xm1_rtos_own_first_slot_164
.size xm1_platform_veneer_own_first_slot_164, .-xm1_platform_veneer_own_first_slot_164

.section .veneer.own_first_slot_168,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_168
.type xm1_platform_veneer_own_first_slot_168,%function
xm1_platform_veneer_own_first_slot_168:
    b xm1_rtos_own_first_slot_168
.size xm1_platform_veneer_own_first_slot_168, .-xm1_platform_veneer_own_first_slot_168

.section .veneer.own_first_slot_16c,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_16c
.type xm1_platform_veneer_own_first_slot_16c,%function
xm1_platform_veneer_own_first_slot_16c:
    b xm1_rtos_own_first_slot_16c
.size xm1_platform_veneer_own_first_slot_16c, .-xm1_platform_veneer_own_first_slot_16c

.section .veneer.own_first_slot_1618,"ax",%progbits
.global xm1_platform_veneer_own_first_slot_1618
.type xm1_platform_veneer_own_first_slot_1618,%function
xm1_platform_veneer_own_first_slot_1618:
    b xm1_rtos_own_first_slot_1618
.size xm1_platform_veneer_own_first_slot_1618, .-xm1_platform_veneer_own_first_slot_1618

.section .veneer.semaphore_create,"ax",%progbits
.global xm1_platform_veneer_semaphore_create
.type xm1_platform_veneer_semaphore_create,%function
xm1_platform_veneer_semaphore_create:
    b xm1_rtos_semaphore_create
.size xm1_platform_veneer_semaphore_create, .-xm1_platform_veneer_semaphore_create

.section .veneer.semaphore_delete,"ax",%progbits
.global xm1_platform_veneer_semaphore_delete
.type xm1_platform_veneer_semaphore_delete,%function
xm1_platform_veneer_semaphore_delete:
    b xm1_rtos_semaphore_delete
.size xm1_platform_veneer_semaphore_delete, .-xm1_platform_veneer_semaphore_delete

.section .veneer.semaphore_signal,"ax",%progbits
.global xm1_platform_veneer_semaphore_signal
.type xm1_platform_veneer_semaphore_signal,%function
xm1_platform_veneer_semaphore_signal:
    b xm1_rtos_semaphore_signal
.size xm1_platform_veneer_semaphore_signal, .-xm1_platform_veneer_semaphore_signal

.section .veneer.semaphore_interrupt_signal,"ax",%progbits
.global xm1_platform_veneer_semaphore_interrupt_signal
.type xm1_platform_veneer_semaphore_interrupt_signal,%function
xm1_platform_veneer_semaphore_interrupt_signal:
    b xm1_rtos_semaphore_interrupt_signal
.size xm1_platform_veneer_semaphore_interrupt_signal, .-xm1_platform_veneer_semaphore_interrupt_signal

.section .veneer.semaphore_wait,"ax",%progbits
.global xm1_platform_veneer_semaphore_wait
.type xm1_platform_veneer_semaphore_wait,%function
xm1_platform_veneer_semaphore_wait:
    b xm1_rtos_semaphore_wait
.size xm1_platform_veneer_semaphore_wait, .-xm1_platform_veneer_semaphore_wait

.section .veneer.semaphore_poll,"ax",%progbits
.global xm1_platform_veneer_semaphore_poll
.type xm1_platform_veneer_semaphore_poll,%function
xm1_platform_veneer_semaphore_poll:
    b xm1_rtos_semaphore_poll
.size xm1_platform_veneer_semaphore_poll, .-xm1_platform_veneer_semaphore_poll

.section .veneer.semaphore_timed_wait,"ax",%progbits
.global xm1_platform_veneer_semaphore_timed_wait
.type xm1_platform_veneer_semaphore_timed_wait,%function
xm1_platform_veneer_semaphore_timed_wait:
    b xm1_rtos_semaphore_timed_wait
.size xm1_platform_veneer_semaphore_timed_wait, .-xm1_platform_veneer_semaphore_timed_wait

.section .veneer.fixed_pool_poll_unchecked,"ax",%progbits
.global xm1_platform_veneer_fixed_pool_poll_unchecked
.type xm1_platform_veneer_fixed_pool_poll_unchecked,%function
xm1_platform_veneer_fixed_pool_poll_unchecked:
    b xm1_rtos_fixed_pool_poll_unchecked
.size xm1_platform_veneer_fixed_pool_poll_unchecked, .-xm1_platform_veneer_fixed_pool_poll_unchecked

.section .veneer.fixed_pool_get,"ax",%progbits
.global xm1_platform_veneer_fixed_pool_get
.type xm1_platform_veneer_fixed_pool_get,%function
xm1_platform_veneer_fixed_pool_get:
    b xm1_rtos_fixed_pool_get
.size xm1_platform_veneer_fixed_pool_get, .-xm1_platform_veneer_fixed_pool_get

.section .veneer.fixed_pool_poll,"ax",%progbits
.global xm1_platform_veneer_fixed_pool_poll
.type xm1_platform_veneer_fixed_pool_poll,%function
xm1_platform_veneer_fixed_pool_poll:
    b xm1_rtos_fixed_pool_poll
.size xm1_platform_veneer_fixed_pool_poll, .-xm1_platform_veneer_fixed_pool_poll

.section .veneer.fixed_pool_timed_get,"ax",%progbits
.global xm1_platform_veneer_fixed_pool_timed_get
.type xm1_platform_veneer_fixed_pool_timed_get,%function
xm1_platform_veneer_fixed_pool_timed_get:
    b xm1_rtos_fixed_pool_timed_get
.size xm1_platform_veneer_fixed_pool_timed_get, .-xm1_platform_veneer_fixed_pool_timed_get

.section .veneer.fixed_pool_release,"ax",%progbits
.global xm1_platform_veneer_fixed_pool_release
.type xm1_platform_veneer_fixed_pool_release,%function
xm1_platform_veneer_fixed_pool_release:
    b xm1_rtos_fixed_pool_release
.size xm1_platform_veneer_fixed_pool_release, .-xm1_platform_veneer_fixed_pool_release

.section .veneer.fixed_pool_create,"ax",%progbits
.global xm1_platform_veneer_fixed_pool_create
.type xm1_platform_veneer_fixed_pool_create,%function
xm1_platform_veneer_fixed_pool_create:
    b xm1_rtos_fixed_pool_create
.size xm1_platform_veneer_fixed_pool_create, .-xm1_platform_veneer_fixed_pool_create

.section .veneer.fixed_pool_delete,"ax",%progbits
.global xm1_platform_veneer_fixed_pool_delete
.type xm1_platform_veneer_fixed_pool_delete,%function
xm1_platform_veneer_fixed_pool_delete:
    b xm1_rtos_fixed_pool_delete
.size xm1_platform_veneer_fixed_pool_delete, .-xm1_platform_veneer_fixed_pool_delete

.section .veneer.task_records_create,"ax",%progbits
.global xm1_platform_veneer_task_records_create
.type xm1_platform_veneer_task_records_create,%function
xm1_platform_veneer_task_records_create:
    b xm1_rtos_task_create
.size xm1_platform_veneer_task_records_create, .-xm1_platform_veneer_task_records_create

.section .veneer.task_records_delete,"ax",%progbits
.global xm1_platform_veneer_task_records_delete
.type xm1_platform_veneer_task_records_delete,%function
xm1_platform_veneer_task_records_delete:
    b xm1_rtos_task_delete
.size xm1_platform_veneer_task_records_delete, .-xm1_platform_veneer_task_records_delete

.section .veneer.global_lock_try,"ax",%progbits
.global xm1_platform_veneer_global_lock_try
.type xm1_platform_veneer_global_lock_try,%function
xm1_platform_veneer_global_lock_try:
    b xm1_rtos_global_lock_try
.size xm1_platform_veneer_global_lock_try, .-xm1_platform_veneer_global_lock_try

.section .veneer.global_lock_release,"ax",%progbits
.global xm1_platform_veneer_global_lock_release
.type xm1_platform_veneer_global_lock_release,%function
xm1_platform_veneer_global_lock_release:
    b xm1_rtos_global_lock_release
.size xm1_platform_veneer_global_lock_release, .-xm1_platform_veneer_global_lock_release

.section .veneer.global_lock_observe,"ax",%progbits
.global xm1_platform_veneer_global_lock_observe
.type xm1_platform_veneer_global_lock_observe,%function
xm1_platform_veneer_global_lock_observe:
    b xm1_rtos_global_lock_observe
.size xm1_platform_veneer_global_lock_observe, .-xm1_platform_veneer_global_lock_observe

.section .veneer.global_lock_helper_try,"ax",%progbits
.global xm1_platform_veneer_global_lock_helper_try
.type xm1_platform_veneer_global_lock_helper_try,%function
xm1_platform_veneer_global_lock_helper_try:
    b helper_try
.size xm1_platform_veneer_global_lock_helper_try, .-xm1_platform_veneer_global_lock_helper_try

.section .veneer.global_lock_helper_release,"ax",%progbits
.global xm1_platform_veneer_global_lock_helper_release
.type xm1_platform_veneer_global_lock_helper_release,%function
xm1_platform_veneer_global_lock_helper_release:
    b helper_release
.size xm1_platform_veneer_global_lock_helper_release, .-xm1_platform_veneer_global_lock_helper_release

.section .veneer.global_lock_helper_observe,"ax",%progbits
.global xm1_platform_veneer_global_lock_helper_observe
.type xm1_platform_veneer_global_lock_helper_observe,%function
xm1_platform_veneer_global_lock_helper_observe:
    b helper_observe
.size xm1_platform_veneer_global_lock_helper_observe, .-xm1_platform_veneer_global_lock_helper_observe


.section .veneer.data_queue_create,"ax",%progbits
.global xm1_platform_veneer_data_queue_create
.type xm1_platform_veneer_data_queue_create,%function
xm1_platform_veneer_data_queue_create:
    b xm1_rtos_data_queue_create
.size xm1_platform_veneer_data_queue_create, .-xm1_platform_veneer_data_queue_create

.section .veneer.data_queue_delete,"ax",%progbits
.global xm1_platform_veneer_data_queue_delete
.type xm1_platform_veneer_data_queue_delete,%function
xm1_platform_veneer_data_queue_delete:
    b xm1_rtos_data_queue_delete
.size xm1_platform_veneer_data_queue_delete, .-xm1_platform_veneer_data_queue_delete

.section .veneer.data_queue_send,"ax",%progbits
.global xm1_platform_veneer_data_queue_send
.type xm1_platform_veneer_data_queue_send,%function
xm1_platform_veneer_data_queue_send:
    b xm1_rtos_data_queue_send
.size xm1_platform_veneer_data_queue_send, .-xm1_platform_veneer_data_queue_send

.section .veneer.data_queue_send_poll,"ax",%progbits
.global xm1_platform_veneer_data_queue_send_poll
.type xm1_platform_veneer_data_queue_send_poll,%function
xm1_platform_veneer_data_queue_send_poll:
    b xm1_rtos_data_queue_send_poll
.size xm1_platform_veneer_data_queue_send_poll, .-xm1_platform_veneer_data_queue_send_poll

.section .veneer.data_queue_send_poll_handler,"ax",%progbits
.global xm1_platform_veneer_data_queue_send_poll_handler
.type xm1_platform_veneer_data_queue_send_poll_handler,%function
xm1_platform_veneer_data_queue_send_poll_handler:
    b xm1_rtos_data_queue_send_poll_handler
.size xm1_platform_veneer_data_queue_send_poll_handler, .-xm1_platform_veneer_data_queue_send_poll_handler

.section .veneer.data_queue_send_timeout,"ax",%progbits
.global xm1_platform_veneer_data_queue_send_timeout
.type xm1_platform_veneer_data_queue_send_timeout,%function
xm1_platform_veneer_data_queue_send_timeout:
    b xm1_rtos_data_queue_send_timeout
.size xm1_platform_veneer_data_queue_send_timeout, .-xm1_platform_veneer_data_queue_send_timeout

.section .veneer.data_queue_force_send,"ax",%progbits
.global xm1_platform_veneer_data_queue_force_send
.type xm1_platform_veneer_data_queue_force_send,%function
xm1_platform_veneer_data_queue_force_send:
    b xm1_rtos_data_queue_force_send
.size xm1_platform_veneer_data_queue_force_send, .-xm1_platform_veneer_data_queue_force_send

.section .veneer.data_queue_force_send_handler,"ax",%progbits
.global xm1_platform_veneer_data_queue_force_send_handler
.type xm1_platform_veneer_data_queue_force_send_handler,%function
xm1_platform_veneer_data_queue_force_send_handler:
    b xm1_rtos_data_queue_force_send_handler
.size xm1_platform_veneer_data_queue_force_send_handler, .-xm1_platform_veneer_data_queue_force_send_handler

.section .veneer.data_queue_receive,"ax",%progbits
.global xm1_platform_veneer_data_queue_receive
.type xm1_platform_veneer_data_queue_receive,%function
xm1_platform_veneer_data_queue_receive:
    b xm1_rtos_data_queue_receive
.size xm1_platform_veneer_data_queue_receive, .-xm1_platform_veneer_data_queue_receive

.section .veneer.data_queue_receive_poll,"ax",%progbits
.global xm1_platform_veneer_data_queue_receive_poll
.type xm1_platform_veneer_data_queue_receive_poll,%function
xm1_platform_veneer_data_queue_receive_poll:
    b xm1_rtos_data_queue_receive_poll
.size xm1_platform_veneer_data_queue_receive_poll, .-xm1_platform_veneer_data_queue_receive_poll

.section .veneer.data_queue_receive_timeout,"ax",%progbits
.global xm1_platform_veneer_data_queue_receive_timeout
.type xm1_platform_veneer_data_queue_receive_timeout,%function
xm1_platform_veneer_data_queue_receive_timeout:
    b xm1_rtos_data_queue_receive_timeout
.size xm1_platform_veneer_data_queue_receive_timeout, .-xm1_platform_veneer_data_queue_receive_timeout
