.syntax unified
.arm
.macro bank_entry role, destination
.section .text.settings_bank_\role\()_entry, "ax", %progbits
.balign 4
.global settings_bank_\role\()_entry
.type settings_bank_\role\()_entry, %function
settings_bank_\role\()_entry:
    b \destination
.size settings_bank_\role\()_entry, .-settings_bank_\role\()_entry
.endm
bank_entry initialize, xm1_storage_initialize
bank_entry copy, xm1_storage_copy
bank_entry capture, xm1_storage_capture
bank_entry restore, xm1_storage_restore
bank_entry exchange, xm1_storage_exchange
bank_entry write_body, xm1_storage_write_body
bank_entry write_active_body, xm1_storage_write_active_body
bank_entry write_header, xm1_storage_write_header
bank_entry read_custom, xm1_storage_read_custom
bank_entry read_body, xm1_storage_read_body
bank_entry read_header, xm1_storage_read_header
bank_entry apply, xm1_storage_apply
