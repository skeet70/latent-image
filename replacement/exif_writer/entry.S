.syntax unified
.arm
.macro entry name
.section .text.exif_entry_\name,"ax",%progbits
.global exif_entry_\name
.type exif_entry_\name,%function
exif_entry_\name:
    b xm1_exif_\name
.size exif_entry_\name, .-exif_entry_\name
.endm
entry initialize
entry write_primary
entry write_exif
entry write_makernote
entry write_gps
entry patch_thumbnail_length
entry transfer_saved_parser
entry swap_halfword
entry copy_value
entry type_size
entry saved_directory_slots
entry saved_directory_values
entry cb_thumbnail_offset
entry cb_thumbnail_length
entry cb_exif_pointer
entry cb_gps_pointer
entry cb_one
entry cb_makernote
entry cb_interoperability_pointer
entry cb_packed_halves
entry cb_source_word
entry cb_staged_block
entry cb_source_bytes_256
entry cb_source_bytes_122
entry cb_default_text
entry cb_copyright
entry cb_printim
entry cb_image_records
entry select_table
entry table_count
entry write_filtered_directory
entry write_exif_header
entry finish
