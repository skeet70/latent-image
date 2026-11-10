    .syntax unified
    .arm

    .section .text.xm1_property_get_entry,"ax",%progbits
    .global xm1_property_get_entry
    .type xm1_property_get_entry,%function
    .p2align 2
xm1_property_get_entry:
    b xm1_property_get
    .size xm1_property_get_entry, . - xm1_property_get_entry

    .section .text.xm1_property_set_entry,"ax",%progbits
    .global xm1_property_set_entry
    .type xm1_property_set_entry,%function
    .p2align 2
xm1_property_set_entry:
    b xm1_property_set
    .size xm1_property_set_entry, . - xm1_property_set_entry

    .section .rodata.property_mapping,"a",%progbits
    .p2align 2
    .local $d.property_rodata
    .type $d.property_rodata,%notype
$d.property_rodata:
    .size $d.property_rodata, . - $d.property_rodata
