#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/arm.h>
#include <unicorn/unicorn.h>

#define GUARD_ENTRY 0x00006000u
#define REPORT_ENTRY 0x00006200u
#define NATIVE_ENTRY 0x00001800u
#define SOURCE 0x00040000u
#define DESTINATION 0x0061c000u
#define BSS_START 0x0089ec00u
#define BSS_END 0x00bf1a00u
#define STACK_TOP 0xfff00f00u
#define FAT_RETURN 0x0000015cu
#define RAW_RETURN 0x00000174u
#define COPY_BYTES (BSS_START - DESTINATION)
#define BSS_BYTES (BSS_END - BSS_START)

typedef struct {
    int report_calls;
    int reached_native;
} Observation;

static void require_uc(uc_err error)
{
    if (error != UC_ERR_OK) {
        fprintf(stderr, "Unicorn: %s\n", uc_strerror(error));
        exit(EXIT_FAILURE);
    }
}

static size_t read_image(const char *path, uint8_t *bytes, size_t capacity)
{
    FILE *file = fopen(path, "rb");
    if (!file) { perror(path); exit(EXIT_FAILURE); }
    size_t length = fread(bytes, 1, capacity, file);
    int extra = fgetc(file);
    if (ferror(file) || extra != EOF || fclose(file) != 0) {
        fprintf(stderr, "Invalid or oversized input: %s\n", path);
        exit(EXIT_FAILURE);
    }
    return length;
}

static void code(uc_engine *uc, uint64_t address, uint32_t size, void *opaque)
{
    (void)size;
    Observation *observation = opaque;
    if (address == REPORT_ENTRY) {
        uint32_t lr;
        uint32_t result = 0;
        observation->report_calls++;
        require_uc(uc_reg_read(uc, UC_ARM_REG_LR, &lr));
        require_uc(uc_reg_write(uc, UC_ARM_REG_R0, &result));
        require_uc(uc_reg_write(uc, UC_ARM_REG_PC, &lr));
    } else if (address == NATIVE_ENTRY) {
        observation->reached_native = 1;
        require_uc(uc_emu_stop(uc));
    }
}

static uc_engine *machine(const uint8_t *guard, size_t guard_length,
                          uint32_t stack, uint32_t return_address,
                          Observation *observation)
{
    uc_engine *uc;
    uc_hook code_hook;
    require_uc(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    require_uc(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    require_uc(uc_mem_map(uc, 0, 0x00c00000, UC_PROT_ALL));
    require_uc(uc_mem_map(uc, 0xfff00000, 0x10000, UC_PROT_ALL));
    require_uc(uc_mem_write(uc, GUARD_ENTRY, guard, guard_length));
    require_uc(uc_mem_write(uc, STACK_TOP - 4, &return_address, sizeof return_address));
    require_uc(uc_reg_write(uc, UC_ARM_REG_SP, &stack));
    uint32_t cpsr = 0x13;
    require_uc(uc_reg_write(uc, UC_ARM_REG_CPSR, &cpsr));
    require_uc(uc_hook_add(uc, &code_hook, UC_HOOK_CODE, (void *)code,
                           observation, 1, 0));
    return uc;
}

static void success(const uint8_t *guard, size_t guard_length)
{
    Observation observation = {0};
    uc_engine *uc = machine(guard, guard_length, STACK_TOP, FAT_RETURN, &observation);
    uint8_t *source = malloc(COPY_BYTES);
    uint8_t *observed = malloc(COPY_BYTES > BSS_BYTES ? COPY_BYTES : BSS_BYTES);
    uint8_t *poison = malloc(BSS_BYTES);
    if (!source || !observed || !poison) { perror("malloc"); exit(EXIT_FAILURE); }
    for (size_t offset = 0; offset < COPY_BYTES; offset++)
        source[offset] = (uint8_t)(offset * 131u + 17u);
    memset(poison, 0xa5, BSS_BYTES);
    require_uc(uc_mem_write(uc, SOURCE, source, COPY_BYTES));
    require_uc(uc_mem_write(uc, BSS_START, poison, BSS_BYTES));
    require_uc(uc_emu_start(uc, GUARD_ENTRY, 0, 0, 5000000));
    if (observation.report_calls != 1 || !observation.reached_native) {
        fprintf(stderr, "accepted route did not report and hand off\n");
        exit(EXIT_FAILURE);
    }
    require_uc(uc_mem_read(uc, DESTINATION, observed, COPY_BYTES));
    if (memcmp(source, observed, COPY_BYTES) != 0) {
        fprintf(stderr, "compact table/segment copy mismatch\n");
        exit(EXIT_FAILURE);
    }
    require_uc(uc_mem_read(uc, BSS_START, observed, BSS_BYTES));
    for (size_t offset = 0; offset < BSS_BYTES; offset++) {
        if (observed[offset] != 0) {
            fprintf(stderr, "segment-4 BSS is not zero at offset %zu\n", offset);
            exit(EXIT_FAILURE);
        }
    }
    free(poison);
    free(observed);
    free(source);
    require_uc(uc_close(uc));
}

static void refusal(const uint8_t *guard, size_t guard_length,
                    uint32_t stack, uint32_t return_address, const char *name)
{
    Observation observation = {0};
    uc_engine *uc = machine(guard, guard_length, stack, return_address, &observation);
    require_uc(uc_emu_start(uc, GUARD_ENTRY, 0, 0, 1000));
    if (observation.report_calls || observation.reached_native) {
        fprintf(stderr, "%s route was admitted\n", name);
        exit(EXIT_FAILURE);
    }
    require_uc(uc_close(uc));
}

int main(int argc, char **argv)
{
    if (argc != 2) {
        fprintf(stderr, "usage: seg4-image-guard-smoke GUARD_BIN\n");
        return EXIT_FAILURE;
    }
    uint8_t guard[0x1400];
    size_t guard_length = read_image(argv[1], guard, sizeof guard);
    if (guard_length == 0) {
        fprintf(stderr, "empty guard\n");
        return EXIT_FAILURE;
    }
    success(guard, guard_length);
    refusal(guard, guard_length, STACK_TOP, RAW_RETURN, "raw");
    refusal(guard, guard_length, STACK_TOP - 4, FAT_RETURN, "wrong-stack");
    puts("PASS: compact copy, BSS clear, FAT handoff and route refusals");
    return EXIT_SUCCESS;
}
