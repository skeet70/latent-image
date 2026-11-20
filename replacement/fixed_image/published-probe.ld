/* The probe image with one section published over stock code below the fixed image. */
INCLUDE probe.ld
MEMORY {
  PUBLISHED (rx) : ORIGIN = 0x700000, LENGTH = 0x1000
}
SECTIONS {
  .published : { KEEP(*(.published)) } > PUBLISHED
}
fixed_test_outside = 0x700100;
