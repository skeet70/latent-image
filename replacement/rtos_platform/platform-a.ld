/* Derived from carriers.json; regenerate when the union changes. */
ENTRY(xm1_platform_veneer_s1_acquire)

MEMORY {
  v_xm1_platform_veneer_s2_irq_doorbell (rx) : ORIGIN = 0x40b00, LENGTH = 0x4
  c_s2_1 (rx) : ORIGIN = 0x40b04, LENGTH = 0x20
  v_xm1_platform_veneer_own_first_slot_164 (rx) : ORIGIN = 0x41140, LENGTH = 0x4
  c_own_first_11 (rx) : ORIGIN = 0x41144, LENGTH = 0x20
  v_xm1_platform_veneer_own_first_slot_168 (rx) : ORIGIN = 0x41164, LENGTH = 0x4
  c_own_first_21 (rx) : ORIGIN = 0x41168, LENGTH = 0x24
  v_xm1_platform_veneer_own_first_slot_16c (rx) : ORIGIN = 0x4118c, LENGTH = 0x4
  c_own_first_31 (rx) : ORIGIN = 0x41190, LENGTH = 0x3c
  v_xm1_platform_veneer_fixed_pool_poll_unchecked (rx) : ORIGIN = 0x411cc, LENGTH = 0x4
  c_fixed_pool_41 (rx) : ORIGIN = 0x411d0, LENGTH = 0xec
  v_xm1_platform_veneer_own_first_slot_1618 (rx) : ORIGIN = 0x412bc, LENGTH = 0x4
  c_own_first_51 (rx) : ORIGIN = 0x412c0, LENGTH = 0xc
  v_xm1_platform_veneer_global_lock_try (rx) : ORIGIN = 0x412cc, LENGTH = 0x4
  c_global_lock_61 (rx) : ORIGIN = 0x412d0, LENGTH = 0x18
  v_xm1_platform_veneer_global_lock_release (rx) : ORIGIN = 0x412e8, LENGTH = 0x4
  c_global_lock_71 (rx) : ORIGIN = 0x412ec, LENGTH = 0x18
  v_xm1_platform_veneer_global_lock_observe (rx) : ORIGIN = 0x41304, LENGTH = 0x4
  c_global_lock_81 (rx) : ORIGIN = 0x41308, LENGTH = 0x18
  v_xm1_platform_veneer_global_lock_helper_try (rx) : ORIGIN = 0x427d4, LENGTH = 0x4
  c_global_lock_91 (rx) : ORIGIN = 0x427d8, LENGTH = 0x24
  v_xm1_platform_veneer_global_lock_helper_release (rx) : ORIGIN = 0x427fc, LENGTH = 0x4
  c_global_lock_101 (rx) : ORIGIN = 0x42800, LENGTH = 0x28
  v_xm1_platform_veneer_global_lock_helper_observe (rx) : ORIGIN = 0x42828, LENGTH = 0x4
  c_global_lock_111 (rx) : ORIGIN = 0x4282c, LENGTH = 0x4
  v_xm1_platform_veneer_s2_dispatch_save (rx) : ORIGIN = 0x43640, LENGTH = 0x4
  c_s2_121 (rx) : ORIGIN = 0x43644, LENGTH = 0x24
  v_xm1_platform_veneer_s2_dispatch_resume (rx) : ORIGIN = 0x43668, LENGTH = 0x4
  c_s2_131 (rx) : ORIGIN = 0x4366c, LENGTH = 0x1c
  v_xm1_platform_veneer_s2_dispatch_no_save (rx) : ORIGIN = 0x43688, LENGTH = 0x4
  c_s2_141 (rx) : ORIGIN = 0x4368c, LENGTH = 0x18
  v_xm1_platform_veneer_s2_dispatch_select (rx) : ORIGIN = 0x436a4, LENGTH = 0x4
  c_s2_142 (rx) : ORIGIN = 0x436a8, LENGTH = 0x80
  v_xm1_platform_veneer_s2_dispatch_first_entry (rx) : ORIGIN = 0x43728, LENGTH = 0x4
  c_s2_151 (rx) : ORIGIN = 0x4372c, LENGTH = 0x30
  v_xm1_platform_veneer_s2_dispatch_irq_return (rx) : ORIGIN = 0x4375c, LENGTH = 0x4
  c_s2_161 (rx) : ORIGIN = 0x43760, LENGTH = 0x44
  v_xm1_platform_veneer_s2_dispatch_irq_resume (rx) : ORIGIN = 0x437a4, LENGTH = 0x4
  v_xm1_platform_veneer_s2_dispatch_irq_tail (rx) : ORIGIN = 0x437a8, LENGTH = 0x4
  c_s2_172 (rx) : ORIGIN = 0x437ac, LENGTH = 0x30
  c_s2_180 (rx) : ORIGIN = 0x437fc, LENGTH = 0x14
  v_xm1_platform_veneer_s1_acquire (rx) : ORIGIN = 0x439d8, LENGTH = 0x4
  c_s1_191 (rx) : ORIGIN = 0x439dc, LENGTH = 0x38
  v_xm1_platform_veneer_s1_release (rx) : ORIGIN = 0x43a14, LENGTH = 0x4
  c_s1_201 (rx) : ORIGIN = 0x43a18, LENGTH = 0x24
  v_xm1_platform_veneer_s1_find (rx) : ORIGIN = 0x43c48, LENGTH = 0x4
  c_s1_211 (rx) : ORIGIN = 0x43c4c, LENGTH = 0x58
  v_xm1_platform_veneer_s1_insert (rx) : ORIGIN = 0x43ca4, LENGTH = 0x4
  c_s1_221 (rx) : ORIGIN = 0x43ca8, LENGTH = 0x78
  v_xm1_platform_veneer_s1_remove (rx) : ORIGIN = 0x43d20, LENGTH = 0x4
  c_s1_231 (rx) : ORIGIN = 0x43d24, LENGTH = 0xac
  v_xm1_platform_veneer_s1_reset (rx) : ORIGIN = 0x43dd0, LENGTH = 0x4
  c_s1_241 (rx) : ORIGIN = 0x43dd4, LENGTH = 0x44
  v_xm1_platform_veneer_s1_prepare (rx) : ORIGIN = 0x43f70, LENGTH = 0x4
  c_s1_251 (rx) : ORIGIN = 0x43f74, LENGTH = 0x28
  v_xm1_platform_veneer_s1_notify (rx) : ORIGIN = 0x44018, LENGTH = 0x4
  c_s1_261 (rx) : ORIGIN = 0x4401c, LENGTH = 0x28
  v_xm1_platform_veneer_s1_requeue (rx) : ORIGIN = 0x44044, LENGTH = 0x4
  c_s1_271 (rx) : ORIGIN = 0x44048, LENGTH = 0xec
  v_xm1_platform_veneer_s1_rotate (rx) : ORIGIN = 0x44134, LENGTH = 0x4
  c_s1_281 (rx) : ORIGIN = 0x44138, LENGTH = 0x7c
  v_xm1_platform_veneer_s3c_s3c_exception_loop (rx) : ORIGIN = 0x441b4, LENGTH = 0x4
  c_s3c_291 (rx) : ORIGIN = 0x441b8, LENGTH = 0xd0
  v_xm1_platform_veneer_s3c_s3c_exception_delivery (rx) : ORIGIN = 0x44288, LENGTH = 0x4
  c_s3c_301 (rx) : ORIGIN = 0x4428c, LENGTH = 0x8c
  v_xm1_platform_veneer_s3b_xm1_s3b_activate (rx) : ORIGIN = 0x44f2c, LENGTH = 0x4
  c_s3b_311 (rx) : ORIGIN = 0x44f30, LENGTH = 0x1c0
  v_xm1_platform_veneer_s3b_xm1_s3b_interrupt_activate (rx) : ORIGIN = 0x450f0, LENGTH = 0x4
  c_s3b_321 (rx) : ORIGIN = 0x450f4, LENGTH = 0x17c
  v_xm1_platform_veneer_s3b_xm1_s3b_cancel_activation (rx) : ORIGIN = 0x45270, LENGTH = 0x4
  c_s3b_331 (rx) : ORIGIN = 0x45274, LENGTH = 0x17c
  v_xm1_platform_veneer_s3a_xm1_s3a_exit_task (rx) : ORIGIN = 0x453f0, LENGTH = 0x4
  c_s3a_341 (rx) : ORIGIN = 0x453f4, LENGTH = 0xfc
  v_xm1_platform_veneer_s3c_s3c_get_priority (rx) : ORIGIN = 0x45b40, LENGTH = 0x4
  c_s3c_351 (rx) : ORIGIN = 0x45b44, LENGTH = 0x190
  v_xm1_platform_veneer_s3c_s3c_sleep (rx) : ORIGIN = 0x45cd4, LENGTH = 0x4
  c_s3c_361 (rx) : ORIGIN = 0x45cd8, LENGTH = 0x17c
  v_xm1_platform_veneer_s3b_xm1_s3b_wake (rx) : ORIGIN = 0x46004, LENGTH = 0x4
  c_s3b_371 (rx) : ORIGIN = 0x46008, LENGTH = 0x1f8
  v_xm1_platform_veneer_s3b_xm1_s3b_interrupt_wake (rx) : ORIGIN = 0x46200, LENGTH = 0x4
  c_s3b_381 (rx) : ORIGIN = 0x46204, LENGTH = 0x194
  v_xm1_platform_veneer_s3b_xm1_s3b_cancel_wake (rx) : ORIGIN = 0x46398, LENGTH = 0x4
  c_s3b_391 (rx) : ORIGIN = 0x4639c, LENGTH = 0x198
  v_xm1_platform_veneer_s3c_s3c_suspend (rx) : ORIGIN = 0x46a00, LENGTH = 0x4
  c_s3c_401 (rx) : ORIGIN = 0x46a04, LENGTH = 0x27c
  v_xm1_platform_veneer_s3c_s3c_resume (rx) : ORIGIN = 0x46c80, LENGTH = 0x4
  c_s3c_411 (rx) : ORIGIN = 0x46c84, LENGTH = 0x1b4
  v_xm1_platform_veneer_s3c_s3c_force_resume (rx) : ORIGIN = 0x46e38, LENGTH = 0x4
  c_s3c_421 (rx) : ORIGIN = 0x46e3c, LENGTH = 0xc
  v_xm1_platform_veneer_s3c_s3c_raise (rx) : ORIGIN = 0x46fcc, LENGTH = 0x4
  c_s3c_431 (rx) : ORIGIN = 0x46fd0, LENGTH = 0x1e0
  v_xm1_platform_veneer_s3c_s3c_interrupt_raise (rx) : ORIGIN = 0x471b0, LENGTH = 0x4
  c_s3c_441 (rx) : ORIGIN = 0x471b4, LENGTH = 0x1a0
  v_xm1_platform_veneer_s3c_s3c_disable_exception (rx) : ORIGIN = 0x47354, LENGTH = 0x4
  c_s3c_451 (rx) : ORIGIN = 0x47358, LENGTH = 0xd0
  v_xm1_platform_veneer_s3c_s3c_enable_exception (rx) : ORIGIN = 0x47428, LENGTH = 0x4
  c_s3c_461 (rx) : ORIGIN = 0x4742c, LENGTH = 0x114
  v_xm1_platform_veneer_own_first_slot_60 (rx) : ORIGIN = 0x47540, LENGTH = 0x4
  c_own_first_471 (rx) : ORIGIN = 0x47544, LENGTH = 0x3c
  v_xm1_platform_veneer_semaphore_signal (rx) : ORIGIN = 0x47604, LENGTH = 0x4
  c_semaphore_481 (rx) : ORIGIN = 0x47608, LENGTH = 0x23c
  v_xm1_platform_veneer_semaphore_interrupt_signal (rx) : ORIGIN = 0x47844, LENGTH = 0x4
  c_semaphore_491 (rx) : ORIGIN = 0x47848, LENGTH = 0x20c
  v_xm1_platform_veneer_semaphore_wait (rx) : ORIGIN = 0x47a54, LENGTH = 0x4
  c_semaphore_501 (rx) : ORIGIN = 0x47a58, LENGTH = 0x23c
  v_xm1_platform_veneer_semaphore_poll (rx) : ORIGIN = 0x47c94, LENGTH = 0x4
  c_semaphore_511 (rx) : ORIGIN = 0x47c98, LENGTH = 0x138
  v_xm1_platform_veneer_semaphore_timed_wait (rx) : ORIGIN = 0x47dd0, LENGTH = 0x4
  c_semaphore_521 (rx) : ORIGIN = 0x47dd4, LENGTH = 0x270
  v_xm1_platform_veneer_fixed_pool_get (rx) : ORIGIN = 0x4b480, LENGTH = 0x4
  c_fixed_pool_531 (rx) : ORIGIN = 0x4b484, LENGTH = 0x24c
  v_xm1_platform_veneer_fixed_pool_poll (rx) : ORIGIN = 0x4b6d0, LENGTH = 0x4
  c_fixed_pool_541 (rx) : ORIGIN = 0x4b6d4, LENGTH = 0x140
  v_xm1_platform_veneer_fixed_pool_timed_get (rx) : ORIGIN = 0x4b814, LENGTH = 0x4
  c_fixed_pool_551 (rx) : ORIGIN = 0x4b818, LENGTH = 0x2a0
  v_xm1_platform_veneer_fixed_pool_release (rx) : ORIGIN = 0x4bab8, LENGTH = 0x4
  c_fixed_pool_561 (rx) : ORIGIN = 0x4babc, LENGTH = 0x290
  v_xm1_platform_veneer_own_first_slot_fc (rx) : ORIGIN = 0x4bd4c, LENGTH = 0x4
  c_own_first_571 (rx) : ORIGIN = 0x4bd50, LENGTH = 0x74
  v_xm1_platform_veneer_own_first_slot_100 (rx) : ORIGIN = 0x4bdc4, LENGTH = 0x4
  c_own_first_581 (rx) : ORIGIN = 0x4bdc8, LENGTH = 0x74
  v_xm1_platform_veneer_s3a_xm1_s3a_rotate_ready (rx) : ORIGIN = 0x4c28c, LENGTH = 0x4
  c_s3a_591 (rx) : ORIGIN = 0x4c290, LENGTH = 0xdc
  v_xm1_platform_veneer_s3a_xm1_s3a_interrupt_rotate_ready (rx) : ORIGIN = 0x4c36c, LENGTH = 0x4
  c_s3a_601 (rx) : ORIGIN = 0x4c370, LENGTH = 0xc4
  v_xm1_platform_veneer_s3a_xm1_s3a_core_rotate_ready (rx) : ORIGIN = 0x4c434, LENGTH = 0x4
  c_s3a_611 (rx) : ORIGIN = 0x4c438, LENGTH = 0xf8
  v_xm1_platform_veneer_s3a_xm1_s3a_interrupt_core_rotate_ready (rx) : ORIGIN = 0x4c530, LENGTH = 0x4
  c_s3a_621 (rx) : ORIGIN = 0x4c534, LENGTH = 0xec
  v_xm1_platform_veneer_own_first_slot_128 (rx) : ORIGIN = 0x4c620, LENGTH = 0x4
  c_own_first_631 (rx) : ORIGIN = 0x4c624, LENGTH = 0x84
  v_xm1_platform_veneer_own_first_slot_12c (rx) : ORIGIN = 0x4c6a8, LENGTH = 0x4
  c_own_first_641 (rx) : ORIGIN = 0x4c6ac, LENGTH = 0x90
  v_xm1_platform_veneer_own_first_slot_130 (rx) : ORIGIN = 0x4c8ac, LENGTH = 0x4
  c_own_first_651 (rx) : ORIGIN = 0x4c8b0, LENGTH = 0x48
  v_xm1_platform_veneer_own_first_slot_134 (rx) : ORIGIN = 0x4c8f8, LENGTH = 0x4
  c_own_first_661 (rx) : ORIGIN = 0x4c8fc, LENGTH = 0x48
  v_xm1_platform_veneer_own_first_slot_138 (rx) : ORIGIN = 0x4c944, LENGTH = 0x4
  c_own_first_671 (rx) : ORIGIN = 0x4c948, LENGTH = 0x48
  v_xm1_platform_veneer_own_first_slot_13c (rx) : ORIGIN = 0x4c990, LENGTH = 0x4
  c_own_first_681 (rx) : ORIGIN = 0x4c994, LENGTH = 0x48
  v_xm1_platform_veneer_s3a_xm1_s3a_disable_dispatch (rx) : ORIGIN = 0x4c9dc, LENGTH = 0x4
  c_s3a_691 (rx) : ORIGIN = 0x4c9e0, LENGTH = 0xe0
  v_xm1_platform_veneer_s3a_xm1_s3a_enable_dispatch (rx) : ORIGIN = 0x4cac0, LENGTH = 0x4
  c_s3a_701 (rx) : ORIGIN = 0x4cac4, LENGTH = 0x114
  v_xm1_platform_veneer_own_first_slot_148 (rx) : ORIGIN = 0x4cbd8, LENGTH = 0x4
  c_own_first_711 (rx) : ORIGIN = 0x4cbdc, LENGTH = 0x24
  v_xm1_platform_veneer_own_first_slot_14c (rx) : ORIGIN = 0x4cc00, LENGTH = 0x4
  c_own_first_721 (rx) : ORIGIN = 0x4cc04, LENGTH = 0x18
  v_xm1_platform_veneer_own_first_slot_150 (rx) : ORIGIN = 0x4cc1c, LENGTH = 0x4
  c_own_first_731 (rx) : ORIGIN = 0x4cc20, LENGTH = 0x24
  v_xm1_platform_veneer_own_first_slot_154 (rx) : ORIGIN = 0x4cc44, LENGTH = 0x4
  c_own_first_741 (rx) : ORIGIN = 0x4cc48, LENGTH = 0x4c
  v_xm1_platform_veneer_task_records_create (rx) : ORIGIN = 0x4d094, LENGTH = 0x4
  c_task_records_751 (rx) : ORIGIN = 0x4d098, LENGTH = 0x10c
  v_xm1_platform_veneer_task_records_delete (rx) : ORIGIN = 0x4d1a4, LENGTH = 0x4
  c_task_records_761 (rx) : ORIGIN = 0x4d1a8, LENGTH = 0x100
  v_xm1_platform_veneer_semaphore_create (rx) : ORIGIN = 0x4d2a8, LENGTH = 0x4
  c_semaphore_771 (rx) : ORIGIN = 0x4d2ac, LENGTH = 0x8c
  v_xm1_platform_veneer_semaphore_delete (rx) : ORIGIN = 0x4d338, LENGTH = 0x4
  c_semaphore_781 (rx) : ORIGIN = 0x4d33c, LENGTH = 0x60
  v_xm1_platform_veneer_fixed_pool_create (rx) : ORIGIN = 0x4d6c4, LENGTH = 0x4
  c_fixed_pool_791 (rx) : ORIGIN = 0x4d6c8, LENGTH = 0xc8
  v_xm1_platform_veneer_fixed_pool_delete (rx) : ORIGIN = 0x4d790, LENGTH = 0x4
  c_fixed_pool_801 (rx) : ORIGIN = 0x4d794, LENGTH = 0x84
}

SECTIONS {
  .veneer_s1_acquire : { KEEP(*(.veneer.s1_acquire)) } > v_xm1_platform_veneer_s1_acquire
  .veneer_s1_release : { KEEP(*(.veneer.s1_release)) } > v_xm1_platform_veneer_s1_release
  .veneer_s1_find : { KEEP(*(.veneer.s1_find)) } > v_xm1_platform_veneer_s1_find
  .veneer_s1_insert : { KEEP(*(.veneer.s1_insert)) } > v_xm1_platform_veneer_s1_insert
  .veneer_s1_remove : { KEEP(*(.veneer.s1_remove)) } > v_xm1_platform_veneer_s1_remove
  .veneer_s1_reset : { KEEP(*(.veneer.s1_reset)) } > v_xm1_platform_veneer_s1_reset
  .veneer_s1_prepare : { KEEP(*(.veneer.s1_prepare)) } > v_xm1_platform_veneer_s1_prepare
  .veneer_s1_notify : { KEEP(*(.veneer.s1_notify)) } > v_xm1_platform_veneer_s1_notify
  .veneer_s1_requeue : { KEEP(*(.veneer.s1_requeue)) } > v_xm1_platform_veneer_s1_requeue
  .veneer_s1_rotate : { KEEP(*(.veneer.s1_rotate)) } > v_xm1_platform_veneer_s1_rotate
  .veneer_s2_irq_doorbell : { KEEP(*(.veneer.s2_irq_doorbell)) } > v_xm1_platform_veneer_s2_irq_doorbell
  .veneer_s2_dispatch_save : { KEEP(*(.veneer.s2_dispatch_save)) } > v_xm1_platform_veneer_s2_dispatch_save
  .veneer_s2_dispatch_resume : { KEEP(*(.veneer.s2_dispatch_resume)) } > v_xm1_platform_veneer_s2_dispatch_resume
  .veneer_s2_dispatch_no_save : { KEEP(*(.veneer.s2_dispatch_no_save)) } > v_xm1_platform_veneer_s2_dispatch_no_save
  .veneer_s2_dispatch_select : { KEEP(*(.veneer.s2_dispatch_select)) } > v_xm1_platform_veneer_s2_dispatch_select
  .veneer_s2_dispatch_first_entry : { KEEP(*(.veneer.s2_dispatch_first_entry)) } > v_xm1_platform_veneer_s2_dispatch_first_entry
  .veneer_s2_dispatch_irq_return : { KEEP(*(.veneer.s2_dispatch_irq_return)) } > v_xm1_platform_veneer_s2_dispatch_irq_return
  .veneer_s2_dispatch_irq_resume : { KEEP(*(.veneer.s2_dispatch_irq_resume)) } > v_xm1_platform_veneer_s2_dispatch_irq_resume
  .veneer_s2_dispatch_irq_tail : { KEEP(*(.veneer.s2_dispatch_irq_tail)) } > v_xm1_platform_veneer_s2_dispatch_irq_tail
  .veneer_s3a_xm1_s3a_exit_task : { KEEP(*(.veneer.s3a_xm1_s3a_exit_task)) } > v_xm1_platform_veneer_s3a_xm1_s3a_exit_task
  .veneer_s3a_xm1_s3a_rotate_ready : { KEEP(*(.veneer.s3a_xm1_s3a_rotate_ready)) } > v_xm1_platform_veneer_s3a_xm1_s3a_rotate_ready
  .veneer_s3a_xm1_s3a_interrupt_rotate_ready : { KEEP(*(.veneer.s3a_xm1_s3a_interrupt_rotate_ready)) } > v_xm1_platform_veneer_s3a_xm1_s3a_interrupt_rotate_ready
  .veneer_s3a_xm1_s3a_core_rotate_ready : { KEEP(*(.veneer.s3a_xm1_s3a_core_rotate_ready)) } > v_xm1_platform_veneer_s3a_xm1_s3a_core_rotate_ready
  .veneer_s3a_xm1_s3a_interrupt_core_rotate_ready : { KEEP(*(.veneer.s3a_xm1_s3a_interrupt_core_rotate_ready)) } > v_xm1_platform_veneer_s3a_xm1_s3a_interrupt_core_rotate_ready
  .veneer_s3a_xm1_s3a_disable_dispatch : { KEEP(*(.veneer.s3a_xm1_s3a_disable_dispatch)) } > v_xm1_platform_veneer_s3a_xm1_s3a_disable_dispatch
  .veneer_s3a_xm1_s3a_enable_dispatch : { KEEP(*(.veneer.s3a_xm1_s3a_enable_dispatch)) } > v_xm1_platform_veneer_s3a_xm1_s3a_enable_dispatch
  .veneer_s3b_xm1_s3b_activate : { KEEP(*(.veneer.s3b_xm1_s3b_activate)) } > v_xm1_platform_veneer_s3b_xm1_s3b_activate
  .veneer_s3b_xm1_s3b_interrupt_activate : { KEEP(*(.veneer.s3b_xm1_s3b_interrupt_activate)) } > v_xm1_platform_veneer_s3b_xm1_s3b_interrupt_activate
  .veneer_s3b_xm1_s3b_cancel_activation : { KEEP(*(.veneer.s3b_xm1_s3b_cancel_activation)) } > v_xm1_platform_veneer_s3b_xm1_s3b_cancel_activation
  .veneer_s3b_xm1_s3b_wake : { KEEP(*(.veneer.s3b_xm1_s3b_wake)) } > v_xm1_platform_veneer_s3b_xm1_s3b_wake
  .veneer_s3b_xm1_s3b_interrupt_wake : { KEEP(*(.veneer.s3b_xm1_s3b_interrupt_wake)) } > v_xm1_platform_veneer_s3b_xm1_s3b_interrupt_wake
  .veneer_s3b_xm1_s3b_cancel_wake : { KEEP(*(.veneer.s3b_xm1_s3b_cancel_wake)) } > v_xm1_platform_veneer_s3b_xm1_s3b_cancel_wake
  .veneer_s3c_s3c_exception_loop : { KEEP(*(.veneer.s3c_s3c_exception_loop)) } > v_xm1_platform_veneer_s3c_s3c_exception_loop
  .veneer_s3c_s3c_exception_delivery : { KEEP(*(.veneer.s3c_s3c_exception_delivery)) } > v_xm1_platform_veneer_s3c_s3c_exception_delivery
  .veneer_s3c_s3c_get_priority : { KEEP(*(.veneer.s3c_s3c_get_priority)) } > v_xm1_platform_veneer_s3c_s3c_get_priority
  .veneer_s3c_s3c_sleep : { KEEP(*(.veneer.s3c_s3c_sleep)) } > v_xm1_platform_veneer_s3c_s3c_sleep
  .veneer_s3c_s3c_suspend : { KEEP(*(.veneer.s3c_s3c_suspend)) } > v_xm1_platform_veneer_s3c_s3c_suspend
  .veneer_s3c_s3c_resume : { KEEP(*(.veneer.s3c_s3c_resume)) } > v_xm1_platform_veneer_s3c_s3c_resume
  .veneer_s3c_s3c_force_resume : { KEEP(*(.veneer.s3c_s3c_force_resume)) } > v_xm1_platform_veneer_s3c_s3c_force_resume
  .veneer_s3c_s3c_raise : { KEEP(*(.veneer.s3c_s3c_raise)) } > v_xm1_platform_veneer_s3c_s3c_raise
  .veneer_s3c_s3c_interrupt_raise : { KEEP(*(.veneer.s3c_s3c_interrupt_raise)) } > v_xm1_platform_veneer_s3c_s3c_interrupt_raise
  .veneer_s3c_s3c_disable_exception : { KEEP(*(.veneer.s3c_s3c_disable_exception)) } > v_xm1_platform_veneer_s3c_s3c_disable_exception
  .veneer_s3c_s3c_enable_exception : { KEEP(*(.veneer.s3c_s3c_enable_exception)) } > v_xm1_platform_veneer_s3c_s3c_enable_exception
  .veneer_own_first_slot_60 : { KEEP(*(.veneer.own_first_slot_60)) } > v_xm1_platform_veneer_own_first_slot_60
  .veneer_own_first_slot_fc : { KEEP(*(.veneer.own_first_slot_fc)) } > v_xm1_platform_veneer_own_first_slot_fc
  .veneer_own_first_slot_100 : { KEEP(*(.veneer.own_first_slot_100)) } > v_xm1_platform_veneer_own_first_slot_100
  .veneer_own_first_slot_128 : { KEEP(*(.veneer.own_first_slot_128)) } > v_xm1_platform_veneer_own_first_slot_128
  .veneer_own_first_slot_12c : { KEEP(*(.veneer.own_first_slot_12c)) } > v_xm1_platform_veneer_own_first_slot_12c
  .veneer_own_first_slot_130 : { KEEP(*(.veneer.own_first_slot_130)) } > v_xm1_platform_veneer_own_first_slot_130
  .veneer_own_first_slot_134 : { KEEP(*(.veneer.own_first_slot_134)) } > v_xm1_platform_veneer_own_first_slot_134
  .veneer_own_first_slot_138 : { KEEP(*(.veneer.own_first_slot_138)) } > v_xm1_platform_veneer_own_first_slot_138
  .veneer_own_first_slot_13c : { KEEP(*(.veneer.own_first_slot_13c)) } > v_xm1_platform_veneer_own_first_slot_13c
  .veneer_own_first_slot_148 : { KEEP(*(.veneer.own_first_slot_148)) } > v_xm1_platform_veneer_own_first_slot_148
  .veneer_own_first_slot_14c : { KEEP(*(.veneer.own_first_slot_14c)) } > v_xm1_platform_veneer_own_first_slot_14c
  .veneer_own_first_slot_150 : { KEEP(*(.veneer.own_first_slot_150)) } > v_xm1_platform_veneer_own_first_slot_150
  .veneer_own_first_slot_154 : { KEEP(*(.veneer.own_first_slot_154)) } > v_xm1_platform_veneer_own_first_slot_154
  .veneer_own_first_slot_164 : { KEEP(*(.veneer.own_first_slot_164)) } > v_xm1_platform_veneer_own_first_slot_164
  .veneer_own_first_slot_168 : { KEEP(*(.veneer.own_first_slot_168)) } > v_xm1_platform_veneer_own_first_slot_168
  .veneer_own_first_slot_16c : { KEEP(*(.veneer.own_first_slot_16c)) } > v_xm1_platform_veneer_own_first_slot_16c
  .veneer_own_first_slot_1618 : { KEEP(*(.veneer.own_first_slot_1618)) } > v_xm1_platform_veneer_own_first_slot_1618
  .veneer_semaphore_create : { KEEP(*(.veneer.semaphore_create)) } > v_xm1_platform_veneer_semaphore_create
  .veneer_semaphore_delete : { KEEP(*(.veneer.semaphore_delete)) } > v_xm1_platform_veneer_semaphore_delete
  .veneer_semaphore_signal : { KEEP(*(.veneer.semaphore_signal)) } > v_xm1_platform_veneer_semaphore_signal
  .veneer_semaphore_interrupt_signal : { KEEP(*(.veneer.semaphore_interrupt_signal)) } > v_xm1_platform_veneer_semaphore_interrupt_signal
  .veneer_semaphore_wait : { KEEP(*(.veneer.semaphore_wait)) } > v_xm1_platform_veneer_semaphore_wait
  .veneer_semaphore_poll : { KEEP(*(.veneer.semaphore_poll)) } > v_xm1_platform_veneer_semaphore_poll
  .veneer_semaphore_timed_wait : { KEEP(*(.veneer.semaphore_timed_wait)) } > v_xm1_platform_veneer_semaphore_timed_wait
  .veneer_fixed_pool_poll_unchecked : { KEEP(*(.veneer.fixed_pool_poll_unchecked)) } > v_xm1_platform_veneer_fixed_pool_poll_unchecked
  .veneer_fixed_pool_get : { KEEP(*(.veneer.fixed_pool_get)) } > v_xm1_platform_veneer_fixed_pool_get
  .veneer_fixed_pool_poll : { KEEP(*(.veneer.fixed_pool_poll)) } > v_xm1_platform_veneer_fixed_pool_poll
  .veneer_fixed_pool_timed_get : { KEEP(*(.veneer.fixed_pool_timed_get)) } > v_xm1_platform_veneer_fixed_pool_timed_get
  .veneer_fixed_pool_release : { KEEP(*(.veneer.fixed_pool_release)) } > v_xm1_platform_veneer_fixed_pool_release
  .veneer_fixed_pool_create : { KEEP(*(.veneer.fixed_pool_create)) } > v_xm1_platform_veneer_fixed_pool_create
  .veneer_fixed_pool_delete : { KEEP(*(.veneer.fixed_pool_delete)) } > v_xm1_platform_veneer_fixed_pool_delete
  .veneer_task_records_create : { KEEP(*(.veneer.task_records_create)) } > v_xm1_platform_veneer_task_records_create
  .veneer_task_records_delete : { KEEP(*(.veneer.task_records_delete)) } > v_xm1_platform_veneer_task_records_delete
  .veneer_global_lock_try : { KEEP(*(.veneer.global_lock_try)) } > v_xm1_platform_veneer_global_lock_try
  .veneer_global_lock_release : { KEEP(*(.veneer.global_lock_release)) } > v_xm1_platform_veneer_global_lock_release
  .veneer_global_lock_observe : { KEEP(*(.veneer.global_lock_observe)) } > v_xm1_platform_veneer_global_lock_observe
  .veneer_global_lock_helper_try : { KEEP(*(.veneer.global_lock_helper_try)) } > v_xm1_platform_veneer_global_lock_helper_try
  .veneer_global_lock_helper_release : { KEEP(*(.veneer.global_lock_helper_release)) } > v_xm1_platform_veneer_global_lock_helper_release
  .veneer_global_lock_helper_observe : { KEEP(*(.veneer.global_lock_helper_observe)) } > v_xm1_platform_veneer_global_lock_helper_observe
  .c_s2_1 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s2_1
  .c_own_first_11 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_11
  .c_own_first_21 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_21
  .c_own_first_31 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_31
  .c_fixed_pool_41 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_fixed_pool_41
  .c_own_first_51 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_51
  .c_global_lock_61 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_global_lock_61
  .c_global_lock_71 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_global_lock_71
  .c_global_lock_81 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_global_lock_81
  .c_global_lock_91 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_global_lock_91
  .c_global_lock_101 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_global_lock_101
  .c_global_lock_111 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_global_lock_111
  .c_s2_121 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s2_121
  .c_s2_131 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s2_131
  .c_s2_141 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s2_141
  .c_s2_142 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s2_142
  .c_s2_151 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s2_151
  .c_s2_161 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s2_161
  .c_s2_172 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s2_172
  .c_s2_180 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s2_180
  .c_s1_191 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_191
  .c_s1_201 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_201
  .c_s1_211 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_211
  .c_s1_221 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_221
  .c_s1_231 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_231
  .c_s1_241 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_241
  .c_s1_251 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_251
  .c_s1_261 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_261
  .c_s1_271 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_271
  .c_s1_281 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s1_281
  .c_s3c_291 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_291
  .c_s3c_301 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_301
  .c_s3b_311 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3b_311
  .c_s3b_321 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3b_321
  .c_s3b_331 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3b_331
  .c_s3a_341 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3a_341
  .c_s3c_351 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_351
  .c_s3c_361 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_361
  .c_s3b_371 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3b_371
  .c_s3b_381 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3b_381
  .c_s3b_391 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3b_391
  .c_s3c_401 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_401
  .c_s3c_411 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_411
  .c_s3c_421 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_421
  .c_s3c_431 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_431
  .c_s3c_441 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_441
  .c_s3c_451 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_451
  .c_s3c_461 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3c_461
  .c_own_first_471 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_471
  .c_semaphore_481 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_semaphore_481
  .c_semaphore_491 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_semaphore_491
  .c_semaphore_501 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_semaphore_501
  .c_semaphore_511 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_semaphore_511
  .c_semaphore_521 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_semaphore_521
  .c_fixed_pool_531 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_fixed_pool_531
  .c_fixed_pool_541 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_fixed_pool_541
  .c_fixed_pool_551 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_fixed_pool_551
  .c_fixed_pool_561 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_fixed_pool_561
  .c_own_first_571 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_571
  .c_own_first_581 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_581
  .c_s3a_591 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3a_591
  .c_s3a_601 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3a_601
  .c_s3a_611 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3a_611
  .c_s3a_621 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3a_621
  .c_own_first_631 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_631
  .c_own_first_641 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_641
  .c_own_first_651 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_651
  .c_own_first_661 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_661
  .c_own_first_671 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_671
  .c_own_first_681 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_681
  .c_s3a_691 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3a_691
  .c_s3a_701 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_s3a_701
  .c_own_first_711 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_711
  .c_own_first_721 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_721
  .c_own_first_731 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_731
  .c_own_first_741 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_own_first_741
  .c_task_records_751 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_task_records_751
  .c_task_records_761 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_task_records_761
  .c_semaphore_771 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_semaphore_771
  .c_semaphore_781 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_semaphore_781
  .c_fixed_pool_791 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_fixed_pool_791
  .c_fixed_pool_801 : { *(.text*) *(.rodata*) . = ALIGN(4); } > c_fixed_pool_801
  .forbidden (NOLOAD) : { *(.data .data.* .bss .bss.* COMMON) } > c_s2_1
  .ARM.attributes 0 : { *(.ARM.attributes) }
  .symtab 0 : { *(.symtab) }
  .shstrtab 0 : { *(.shstrtab) }
  .strtab 0 : { *(.strtab) }
  /DISCARD/ : { *(.ARM.exidx .ARM.exidx.* .ARM.extab .ARM.extab.* .comment .note .note.* .debug* .llvm_addrsig) }
}

xm1_rtos_scheduler_stock_raise_irq = 0x433e4;
s3a_stock_remove = 0x43d20;
s3a_stock_reset = 0x43dd0;
s3a_stock_prepare = 0x43f70;
s3a_stock_rotate = 0x44134;
s3a_stock_notify = 0x44018;
s3a_stock_save = 0x43640;
s3a_stock_discard = 0x43688;
s3b_prepare = 0x43f70;
s3b_release_wait = 0x44568;
s3b_notify = 0x44018;
s3b_switch = 0x43640;
s3c_remove = 0x43d20;
s3c_insert = 0x43ca4;
s3c_notify = 0x44018;
s3c_switch = 0x43640;
xm1_rtos_scheduler_s2_exit_task = 0x453f0;
xm1_rtos_scheduler_s2_deliver_exception = 0x44288;
xm1_rtos_scheduler_s2_clear_irq = 0x43444;
xm1_rtos_scheduler_s2_global_unlock = 0x427fc;
xm1_semaphore_stock_acquire = 0x439d8;
xm1_semaphore_stock_release = 0x43a14;
xm1_semaphore_stock_notify = 0x44018;
xm1_semaphore_stock_dispatch = 0x43640;
xm1_semaphore_stock_mask = 0x42560;
xm1_semaphore_stock_restore = 0x42568;
xm1_semaphore_stock_wait_release = 0x44568;
xm1_semaphore_stock_wait_cancel = 0x44744;
xm1_semaphore_stock_wait_enqueue = 0x44800;
xm1_semaphore_stock_wait_enqueue_timed = 0x448e4;
xm1_fixed_pool_stock_acquire = 0x439d8;
xm1_fixed_pool_stock_release = 0x43a14;
xm1_fixed_pool_stock_notify = 0x44018;
xm1_fixed_pool_stock_dispatch = 0x43640;
xm1_fixed_pool_stock_mask = 0x42560;
xm1_fixed_pool_stock_restore = 0x42568;
xm1_fixed_pool_stock_save_irq = 0x42574;
xm1_fixed_pool_stock_restore_irq = 0x4257c;
xm1_fixed_pool_stock_wait_release = 0x44568;
xm1_fixed_pool_stock_wait_cancel = 0x44744;
xm1_fixed_pool_stock_wait_enqueue = 0x44800;
xm1_fixed_pool_stock_wait_enqueue_timed = 0x448e4;
xm1_fixed_pool_stock_uidivmod = 0x4da6c;
xm1_task_mask_save = 0x42560;
xm1_task_mask_restore = 0x42568;

ASSERT(xm1_platform_veneer_s1_acquire == 0x439d8, "veneer s1_acquire left its stock entry")
ASSERT(SIZEOF(.veneer_s1_acquire) == 4, "veneer s1_acquire is not one branch")
ASSERT(xm1_platform_veneer_s1_release == 0x43a14, "veneer s1_release left its stock entry")
ASSERT(SIZEOF(.veneer_s1_release) == 4, "veneer s1_release is not one branch")
ASSERT(xm1_platform_veneer_s1_find == 0x43c48, "veneer s1_find left its stock entry")
ASSERT(SIZEOF(.veneer_s1_find) == 4, "veneer s1_find is not one branch")
ASSERT(xm1_platform_veneer_s1_insert == 0x43ca4, "veneer s1_insert left its stock entry")
ASSERT(SIZEOF(.veneer_s1_insert) == 4, "veneer s1_insert is not one branch")
ASSERT(xm1_platform_veneer_s1_remove == 0x43d20, "veneer s1_remove left its stock entry")
ASSERT(SIZEOF(.veneer_s1_remove) == 4, "veneer s1_remove is not one branch")
ASSERT(xm1_platform_veneer_s1_reset == 0x43dd0, "veneer s1_reset left its stock entry")
ASSERT(SIZEOF(.veneer_s1_reset) == 4, "veneer s1_reset is not one branch")
ASSERT(xm1_platform_veneer_s1_prepare == 0x43f70, "veneer s1_prepare left its stock entry")
ASSERT(SIZEOF(.veneer_s1_prepare) == 4, "veneer s1_prepare is not one branch")
ASSERT(xm1_platform_veneer_s1_notify == 0x44018, "veneer s1_notify left its stock entry")
ASSERT(SIZEOF(.veneer_s1_notify) == 4, "veneer s1_notify is not one branch")
ASSERT(xm1_platform_veneer_s1_requeue == 0x44044, "veneer s1_requeue left its stock entry")
ASSERT(SIZEOF(.veneer_s1_requeue) == 4, "veneer s1_requeue is not one branch")
ASSERT(xm1_platform_veneer_s1_rotate == 0x44134, "veneer s1_rotate left its stock entry")
ASSERT(SIZEOF(.veneer_s1_rotate) == 4, "veneer s1_rotate is not one branch")
ASSERT(xm1_platform_veneer_s2_irq_doorbell == 0x40b00, "veneer s2_irq_doorbell left its stock entry")
ASSERT(SIZEOF(.veneer_s2_irq_doorbell) == 4, "veneer s2_irq_doorbell is not one branch")
ASSERT(xm1_platform_veneer_s2_dispatch_save == 0x43640, "veneer s2_dispatch_save left its stock entry")
ASSERT(SIZEOF(.veneer_s2_dispatch_save) == 4, "veneer s2_dispatch_save is not one branch")
ASSERT(xm1_platform_veneer_s2_dispatch_resume == 0x43668, "veneer s2_dispatch_resume left its stock entry")
ASSERT(SIZEOF(.veneer_s2_dispatch_resume) == 4, "veneer s2_dispatch_resume is not one branch")
ASSERT(xm1_platform_veneer_s2_dispatch_no_save == 0x43688, "veneer s2_dispatch_no_save left its stock entry")
ASSERT(SIZEOF(.veneer_s2_dispatch_no_save) == 4, "veneer s2_dispatch_no_save is not one branch")
ASSERT(xm1_platform_veneer_s2_dispatch_select == 0x436a4, "veneer s2_dispatch_select left its stock entry")
ASSERT(SIZEOF(.veneer_s2_dispatch_select) == 4, "veneer s2_dispatch_select is not one branch")
ASSERT(xm1_platform_veneer_s2_dispatch_first_entry == 0x43728, "veneer s2_dispatch_first_entry left its stock entry")
ASSERT(SIZEOF(.veneer_s2_dispatch_first_entry) == 4, "veneer s2_dispatch_first_entry is not one branch")
ASSERT(xm1_platform_veneer_s2_dispatch_irq_return == 0x4375c, "veneer s2_dispatch_irq_return left its stock entry")
ASSERT(SIZEOF(.veneer_s2_dispatch_irq_return) == 4, "veneer s2_dispatch_irq_return is not one branch")
ASSERT(xm1_platform_veneer_s2_dispatch_irq_resume == 0x437a4, "veneer s2_dispatch_irq_resume left its stock entry")
ASSERT(SIZEOF(.veneer_s2_dispatch_irq_resume) == 4, "veneer s2_dispatch_irq_resume is not one branch")
ASSERT(xm1_platform_veneer_s2_dispatch_irq_tail == 0x437a8, "veneer s2_dispatch_irq_tail left its stock entry")
ASSERT(SIZEOF(.veneer_s2_dispatch_irq_tail) == 4, "veneer s2_dispatch_irq_tail is not one branch")
ASSERT(xm1_platform_veneer_s3a_xm1_s3a_exit_task == 0x453f0, "veneer s3a_xm1_s3a_exit_task left its stock entry")
ASSERT(SIZEOF(.veneer_s3a_xm1_s3a_exit_task) == 4, "veneer s3a_xm1_s3a_exit_task is not one branch")
ASSERT(xm1_platform_veneer_s3a_xm1_s3a_rotate_ready == 0x4c28c, "veneer s3a_xm1_s3a_rotate_ready left its stock entry")
ASSERT(SIZEOF(.veneer_s3a_xm1_s3a_rotate_ready) == 4, "veneer s3a_xm1_s3a_rotate_ready is not one branch")
ASSERT(xm1_platform_veneer_s3a_xm1_s3a_interrupt_rotate_ready == 0x4c36c, "veneer s3a_xm1_s3a_interrupt_rotate_ready left its stock entry")
ASSERT(SIZEOF(.veneer_s3a_xm1_s3a_interrupt_rotate_ready) == 4, "veneer s3a_xm1_s3a_interrupt_rotate_ready is not one branch")
ASSERT(xm1_platform_veneer_s3a_xm1_s3a_core_rotate_ready == 0x4c434, "veneer s3a_xm1_s3a_core_rotate_ready left its stock entry")
ASSERT(SIZEOF(.veneer_s3a_xm1_s3a_core_rotate_ready) == 4, "veneer s3a_xm1_s3a_core_rotate_ready is not one branch")
ASSERT(xm1_platform_veneer_s3a_xm1_s3a_interrupt_core_rotate_ready == 0x4c530, "veneer s3a_xm1_s3a_interrupt_core_rotate_ready left its stock entry")
ASSERT(SIZEOF(.veneer_s3a_xm1_s3a_interrupt_core_rotate_ready) == 4, "veneer s3a_xm1_s3a_interrupt_core_rotate_ready is not one branch")
ASSERT(xm1_platform_veneer_s3a_xm1_s3a_disable_dispatch == 0x4c9dc, "veneer s3a_xm1_s3a_disable_dispatch left its stock entry")
ASSERT(SIZEOF(.veneer_s3a_xm1_s3a_disable_dispatch) == 4, "veneer s3a_xm1_s3a_disable_dispatch is not one branch")
ASSERT(xm1_platform_veneer_s3a_xm1_s3a_enable_dispatch == 0x4cac0, "veneer s3a_xm1_s3a_enable_dispatch left its stock entry")
ASSERT(SIZEOF(.veneer_s3a_xm1_s3a_enable_dispatch) == 4, "veneer s3a_xm1_s3a_enable_dispatch is not one branch")
ASSERT(xm1_platform_veneer_s3b_xm1_s3b_activate == 0x44f2c, "veneer s3b_xm1_s3b_activate left its stock entry")
ASSERT(SIZEOF(.veneer_s3b_xm1_s3b_activate) == 4, "veneer s3b_xm1_s3b_activate is not one branch")
ASSERT(xm1_platform_veneer_s3b_xm1_s3b_interrupt_activate == 0x450f0, "veneer s3b_xm1_s3b_interrupt_activate left its stock entry")
ASSERT(SIZEOF(.veneer_s3b_xm1_s3b_interrupt_activate) == 4, "veneer s3b_xm1_s3b_interrupt_activate is not one branch")
ASSERT(xm1_platform_veneer_s3b_xm1_s3b_cancel_activation == 0x45270, "veneer s3b_xm1_s3b_cancel_activation left its stock entry")
ASSERT(SIZEOF(.veneer_s3b_xm1_s3b_cancel_activation) == 4, "veneer s3b_xm1_s3b_cancel_activation is not one branch")
ASSERT(xm1_platform_veneer_s3b_xm1_s3b_wake == 0x46004, "veneer s3b_xm1_s3b_wake left its stock entry")
ASSERT(SIZEOF(.veneer_s3b_xm1_s3b_wake) == 4, "veneer s3b_xm1_s3b_wake is not one branch")
ASSERT(xm1_platform_veneer_s3b_xm1_s3b_interrupt_wake == 0x46200, "veneer s3b_xm1_s3b_interrupt_wake left its stock entry")
ASSERT(SIZEOF(.veneer_s3b_xm1_s3b_interrupt_wake) == 4, "veneer s3b_xm1_s3b_interrupt_wake is not one branch")
ASSERT(xm1_platform_veneer_s3b_xm1_s3b_cancel_wake == 0x46398, "veneer s3b_xm1_s3b_cancel_wake left its stock entry")
ASSERT(SIZEOF(.veneer_s3b_xm1_s3b_cancel_wake) == 4, "veneer s3b_xm1_s3b_cancel_wake is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_exception_loop == 0x441b4, "veneer s3c_s3c_exception_loop left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_exception_loop) == 4, "veneer s3c_s3c_exception_loop is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_exception_delivery == 0x44288, "veneer s3c_s3c_exception_delivery left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_exception_delivery) == 4, "veneer s3c_s3c_exception_delivery is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_get_priority == 0x45b40, "veneer s3c_s3c_get_priority left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_get_priority) == 4, "veneer s3c_s3c_get_priority is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_sleep == 0x45cd4, "veneer s3c_s3c_sleep left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_sleep) == 4, "veneer s3c_s3c_sleep is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_suspend == 0x46a00, "veneer s3c_s3c_suspend left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_suspend) == 4, "veneer s3c_s3c_suspend is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_resume == 0x46c80, "veneer s3c_s3c_resume left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_resume) == 4, "veneer s3c_s3c_resume is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_force_resume == 0x46e38, "veneer s3c_s3c_force_resume left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_force_resume) == 4, "veneer s3c_s3c_force_resume is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_raise == 0x46fcc, "veneer s3c_s3c_raise left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_raise) == 4, "veneer s3c_s3c_raise is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_interrupt_raise == 0x471b0, "veneer s3c_s3c_interrupt_raise left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_interrupt_raise) == 4, "veneer s3c_s3c_interrupt_raise is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_disable_exception == 0x47354, "veneer s3c_s3c_disable_exception left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_disable_exception) == 4, "veneer s3c_s3c_disable_exception is not one branch")
ASSERT(xm1_platform_veneer_s3c_s3c_enable_exception == 0x47428, "veneer s3c_s3c_enable_exception left its stock entry")
ASSERT(SIZEOF(.veneer_s3c_s3c_enable_exception) == 4, "veneer s3c_s3c_enable_exception is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_60 == 0x47540, "veneer own_first_slot_60 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_60) == 4, "veneer own_first_slot_60 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_fc == 0x4bd4c, "veneer own_first_slot_fc left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_fc) == 4, "veneer own_first_slot_fc is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_100 == 0x4bdc4, "veneer own_first_slot_100 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_100) == 4, "veneer own_first_slot_100 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_128 == 0x4c620, "veneer own_first_slot_128 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_128) == 4, "veneer own_first_slot_128 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_12c == 0x4c6a8, "veneer own_first_slot_12c left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_12c) == 4, "veneer own_first_slot_12c is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_130 == 0x4c8ac, "veneer own_first_slot_130 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_130) == 4, "veneer own_first_slot_130 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_134 == 0x4c8f8, "veneer own_first_slot_134 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_134) == 4, "veneer own_first_slot_134 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_138 == 0x4c944, "veneer own_first_slot_138 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_138) == 4, "veneer own_first_slot_138 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_13c == 0x4c990, "veneer own_first_slot_13c left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_13c) == 4, "veneer own_first_slot_13c is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_148 == 0x4cbd8, "veneer own_first_slot_148 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_148) == 4, "veneer own_first_slot_148 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_14c == 0x4cc00, "veneer own_first_slot_14c left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_14c) == 4, "veneer own_first_slot_14c is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_150 == 0x4cc1c, "veneer own_first_slot_150 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_150) == 4, "veneer own_first_slot_150 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_154 == 0x4cc44, "veneer own_first_slot_154 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_154) == 4, "veneer own_first_slot_154 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_164 == 0x41140, "veneer own_first_slot_164 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_164) == 4, "veneer own_first_slot_164 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_168 == 0x41164, "veneer own_first_slot_168 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_168) == 4, "veneer own_first_slot_168 is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_16c == 0x4118c, "veneer own_first_slot_16c left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_16c) == 4, "veneer own_first_slot_16c is not one branch")
ASSERT(xm1_platform_veneer_own_first_slot_1618 == 0x412bc, "veneer own_first_slot_1618 left its stock entry")
ASSERT(SIZEOF(.veneer_own_first_slot_1618) == 4, "veneer own_first_slot_1618 is not one branch")
ASSERT(xm1_platform_veneer_semaphore_create == 0x4d2a8, "veneer semaphore_create left its stock entry")
ASSERT(SIZEOF(.veneer_semaphore_create) == 4, "veneer semaphore_create is not one branch")
ASSERT(xm1_platform_veneer_semaphore_delete == 0x4d338, "veneer semaphore_delete left its stock entry")
ASSERT(SIZEOF(.veneer_semaphore_delete) == 4, "veneer semaphore_delete is not one branch")
ASSERT(xm1_platform_veneer_semaphore_signal == 0x47604, "veneer semaphore_signal left its stock entry")
ASSERT(SIZEOF(.veneer_semaphore_signal) == 4, "veneer semaphore_signal is not one branch")
ASSERT(xm1_platform_veneer_semaphore_interrupt_signal == 0x47844, "veneer semaphore_interrupt_signal left its stock entry")
ASSERT(SIZEOF(.veneer_semaphore_interrupt_signal) == 4, "veneer semaphore_interrupt_signal is not one branch")
ASSERT(xm1_platform_veneer_semaphore_wait == 0x47a54, "veneer semaphore_wait left its stock entry")
ASSERT(SIZEOF(.veneer_semaphore_wait) == 4, "veneer semaphore_wait is not one branch")
ASSERT(xm1_platform_veneer_semaphore_poll == 0x47c94, "veneer semaphore_poll left its stock entry")
ASSERT(SIZEOF(.veneer_semaphore_poll) == 4, "veneer semaphore_poll is not one branch")
ASSERT(xm1_platform_veneer_semaphore_timed_wait == 0x47dd0, "veneer semaphore_timed_wait left its stock entry")
ASSERT(SIZEOF(.veneer_semaphore_timed_wait) == 4, "veneer semaphore_timed_wait is not one branch")
ASSERT(xm1_platform_veneer_fixed_pool_poll_unchecked == 0x411cc, "veneer fixed_pool_poll_unchecked left its stock entry")
ASSERT(SIZEOF(.veneer_fixed_pool_poll_unchecked) == 4, "veneer fixed_pool_poll_unchecked is not one branch")
ASSERT(xm1_platform_veneer_fixed_pool_get == 0x4b480, "veneer fixed_pool_get left its stock entry")
ASSERT(SIZEOF(.veneer_fixed_pool_get) == 4, "veneer fixed_pool_get is not one branch")
ASSERT(xm1_platform_veneer_fixed_pool_poll == 0x4b6d0, "veneer fixed_pool_poll left its stock entry")
ASSERT(SIZEOF(.veneer_fixed_pool_poll) == 4, "veneer fixed_pool_poll is not one branch")
ASSERT(xm1_platform_veneer_fixed_pool_timed_get == 0x4b814, "veneer fixed_pool_timed_get left its stock entry")
ASSERT(SIZEOF(.veneer_fixed_pool_timed_get) == 4, "veneer fixed_pool_timed_get is not one branch")
ASSERT(xm1_platform_veneer_fixed_pool_release == 0x4bab8, "veneer fixed_pool_release left its stock entry")
ASSERT(SIZEOF(.veneer_fixed_pool_release) == 4, "veneer fixed_pool_release is not one branch")
ASSERT(xm1_platform_veneer_fixed_pool_create == 0x4d6c4, "veneer fixed_pool_create left its stock entry")
ASSERT(SIZEOF(.veneer_fixed_pool_create) == 4, "veneer fixed_pool_create is not one branch")
ASSERT(xm1_platform_veneer_fixed_pool_delete == 0x4d790, "veneer fixed_pool_delete left its stock entry")
ASSERT(SIZEOF(.veneer_fixed_pool_delete) == 4, "veneer fixed_pool_delete is not one branch")
ASSERT(xm1_platform_veneer_task_records_create == 0x4d094, "veneer task_records_create left its stock entry")
ASSERT(SIZEOF(.veneer_task_records_create) == 4, "veneer task_records_create is not one branch")
ASSERT(xm1_platform_veneer_task_records_delete == 0x4d1a4, "veneer task_records_delete left its stock entry")
ASSERT(SIZEOF(.veneer_task_records_delete) == 4, "veneer task_records_delete is not one branch")
ASSERT(xm1_platform_veneer_global_lock_try == 0x412cc, "veneer global_lock_try left its stock entry")
ASSERT(SIZEOF(.veneer_global_lock_try) == 4, "veneer global_lock_try is not one branch")
ASSERT(xm1_platform_veneer_global_lock_release == 0x412e8, "veneer global_lock_release left its stock entry")
ASSERT(SIZEOF(.veneer_global_lock_release) == 4, "veneer global_lock_release is not one branch")
ASSERT(xm1_platform_veneer_global_lock_observe == 0x41304, "veneer global_lock_observe left its stock entry")
ASSERT(SIZEOF(.veneer_global_lock_observe) == 4, "veneer global_lock_observe is not one branch")
ASSERT(xm1_platform_veneer_global_lock_helper_try == 0x427d4, "veneer global_lock_helper_try left its stock entry")
ASSERT(SIZEOF(.veneer_global_lock_helper_try) == 4, "veneer global_lock_helper_try is not one branch")
ASSERT(xm1_platform_veneer_global_lock_helper_release == 0x427fc, "veneer global_lock_helper_release left its stock entry")
ASSERT(SIZEOF(.veneer_global_lock_helper_release) == 4, "veneer global_lock_helper_release is not one branch")
ASSERT(xm1_platform_veneer_global_lock_helper_observe == 0x42828, "veneer global_lock_helper_observe left its stock entry")
ASSERT(SIZEOF(.veneer_global_lock_helper_observe) == 4, "veneer global_lock_helper_observe is not one branch")
ASSERT(SIZEOF(.forbidden) == 0, "data or bss present")
