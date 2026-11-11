.syntax unified
.arm
.text
.global xm1_profile_load_optional_rate_byte
.type xm1_profile_load_optional_rate_byte,%function
xm1_profile_load_optional_rate_byte:
    ldrb r0, [r0]
    bx lr
.size xm1_profile_load_optional_rate_byte, .-xm1_profile_load_optional_rate_byte

.global xm1_profile_load_physical_word
.type xm1_profile_load_physical_word,%function
xm1_profile_load_physical_word:
    ldr r0, [r0]
    bx lr
.size xm1_profile_load_physical_word, .-xm1_profile_load_physical_word

.global xm1_profile_cache_all
.type xm1_profile_cache_all,%function
xm1_profile_cache_all:
    mov r0, #0
    mov r1, #0
    ldr pc, =0x00042634
.size xm1_profile_cache_all, .-xm1_profile_cache_all
