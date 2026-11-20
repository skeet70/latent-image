/* The probe image with one section published over stock code above the fixed image. */
INCLUDE probe.ld
MEMORY {
  PUBLISHED (rx) : ORIGIN = 0xefc000, LENGTH = 0x1000
}
SECTIONS {
  .published : { KEEP(*(.published)) } > PUBLISHED
}
fixed_test_outside = 0xefc100;
