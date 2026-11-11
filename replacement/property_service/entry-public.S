    .syntax unified
    .arm

    .section .text.xm1_public_get,"ax",%progbits
    .global xm1_public_get
    .type xm1_public_get,%function
    .p2align 2
xm1_public_get:
    push {r4, r5, r11, lr}
    bl xm1_property_get
    pop {r4, r5, r11, pc}
    .size xm1_public_get, . - xm1_public_get

    .section .text.xm1_public_set,"ax",%progbits
    .global xm1_public_set
    .type xm1_public_set,%function
    .p2align 2
xm1_public_set:
    push {r3, r4, r5, r6, r7, r8, r11, lr}
    bl xm1_property_set
    pop {r3, r4, r5, r6, r7, r8, r11, pc}
    .size xm1_public_set, . - xm1_public_set

    .section .rodata.property_mapping,"a",%progbits
    .p2align 2
    .local $d.property_rodata
    .type $d.property_rodata,%notype
$d.property_rodata:
    .size $d.property_rodata, . - $d.property_rodata
