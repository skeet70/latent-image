.syntax unified
.arm
.macro entry name
.section .text.record_store_entry_\name,"ax",%progbits
.global record_store_entry_\name
.type record_store_entry_\name,%function
record_store_entry_\name:
    b record_store_\name
.size record_store_entry_\name, .-record_store_entry_\name
.endm
entry open
entry register
entry close
entry read_latest
entry append
entry erase_all
entry collect
entry read_at
entry zero
