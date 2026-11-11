.syntax unified
.arm
.text
.align 2
.global xm1_encoder_cache
.type xm1_encoder_cache,%function
xm1_encoder_cache:
    ldr pc, =0x00042634
.size xm1_encoder_cache, .-xm1_encoder_cache
.global xm1_encoder_save_irq
.type xm1_encoder_save_irq,%function
xm1_encoder_save_irq:
    ldr pc, =0x00042574
.size xm1_encoder_save_irq, .-xm1_encoder_save_irq
.global xm1_encoder_restore_irq
.type xm1_encoder_restore_irq,%function
xm1_encoder_restore_irq:
    ldr pc, =0x0004257c
.size xm1_encoder_restore_irq, .-xm1_encoder_restore_irq
.section .note.GNU-stack,"",%progbits
