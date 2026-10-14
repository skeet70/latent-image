#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/arm.h>
#include <unicorn/unicorn.h>

#define GUARD 0x00006000u
#define CALLBACK 0x000060c0u
#define STAGE 0x00006200u
#define RESET 0x00001800u
#define CLOSE 0x00002044u
#define CLASSIFIER 0x00001aa0u
#define SOURCE 0x00040000u
#define SOURCE_SIZE 0x00020000u
#define WRITER 0x006a9268u
#define HELPER 0x00723a14u
#define TAP 0x0071feb4u
#define COMPLETION 0x0073b564u
#define READY 0x00828574u
#define STOCK 0xebffffdcu
#define RECORDER 0xeb34a9c9u
#define COMPLETION_WORD 0xebffa12au
#define REPAIR_BRANCH 0xebe32b25u
#define ROM_STACK 0xfff00f00u
#define STACK_BASE 0x3b000000u
#define STACK_TOP (STACK_BASE + 0x1000u)
#define LED_DATA 0xfff893c0u

typedef enum { RUNNING, RESET_REACHED, CLOSE_REACHED, HELPER_REACHED, REFUSED } Outcome;
typedef struct {
    Outcome outcome;
    uint32_t writer_status;
    unsigned writer_calls;
    unsigned led_writes;
    unsigned stop_led_writes;
    int bad;
} Run;

static void checked(uc_err error) {
    if (error != UC_ERR_OK) {
        fprintf(stderr, "Unicorn: %s\n", uc_strerror(error));
        exit(EXIT_FAILURE);
    }
}

static uint32_t reg(uc_engine *uc, int id) {
    uint32_t value;
    checked(uc_reg_read(uc, id, &value));
    return value;
}

static void set_reg(uc_engine *uc, int id, uint32_t value) {
    checked(uc_reg_write(uc, id, &value));
}

static uint32_t crc32(const uint8_t *bytes, size_t count) {
    uint32_t crc = UINT32_MAX;
    for (size_t i = 0; i < count; i++) {
        crc ^= bytes[i];
        for (unsigned bit = 0; bit < 8; bit++)
            crc = (crc >> 1) ^ (0xedb88320u & (0u - (crc & 1u)));
    }
    return ~crc;
}

static void make_source(uint8_t *bytes) {
    const uint32_t wanted = 0x92863e8bu;
    memset(bytes, 0, SOURCE_SIZE);
    uint32_t base = crc32(bytes, SOURCE_SIZE);
    uint32_t basis[32] = {0};
    uint32_t combinations[32] = {0};
    for (unsigned input = 0; input < 32; input++) {
        bytes[SOURCE_SIZE - 4 + input / 8] ^= (uint8_t)(1u << (input % 8));
        uint32_t value = crc32(bytes, SOURCE_SIZE) ^ base;
        bytes[SOURCE_SIZE - 4 + input / 8] ^= (uint8_t)(1u << (input % 8));
        uint32_t combination = 1u << input;
        for (int output = 31; output >= 0; output--) {
            if ((value & (1u << output)) == 0) continue;
            if (basis[output] == 0) {
                basis[output] = value;
                combinations[output] = combination;
                break;
            }
            value ^= basis[output];
            combination ^= combinations[output];
        }
    }
    uint32_t target = wanted ^ base;
    uint32_t selected = 0;
    for (int output = 31; output >= 0; output--) {
        if ((target & (1u << output)) == 0) continue;
        if (basis[output] == 0) exit(EXIT_FAILURE);
        target ^= basis[output];
        selected ^= combinations[output];
    }
    if (target != 0) exit(EXIT_FAILURE);
    for (unsigned input = 0; input < 32; input++)
        if (selected & (1u << input))
            bytes[SOURCE_SIZE - 4 + input / 8] ^= (uint8_t)(1u << (input % 8));
    if (crc32(bytes, SOURCE_SIZE) != wanted) exit(EXIT_FAILURE);
}

static void code(uc_engine *uc, uint64_t pc, uint32_t size, void *opaque) {
    (void)size;
    Run *run = opaque;
    uint32_t word = 0;
    checked(uc_mem_read(uc, pc, &word, sizeof word));
    if (word == 0xee070f3au || word == 0xee070f35u || word == 0xee070f15u) {
        set_reg(uc, UC_ARM_REG_PC, (uint32_t)pc + 4);
    } else if (word == 0xe2500001u) {
        set_reg(uc, UC_ARM_REG_R0, 1);
    } else if (pc == RESET) {
        run->outcome = RESET_REACHED;
        checked(uc_emu_stop(uc));
    } else if (pc == CLOSE) {
        run->outcome = CLOSE_REACHED;
        checked(uc_emu_stop(uc));
    } else if (pc == WRITER) {
        run->writer_calls++;
        if (reg(uc, UC_ARM_REG_R0) != 14 || reg(uc, UC_ARM_REG_R1) != 1 ||
            reg(uc, UC_ARM_REG_R2) != SOURCE) run->bad = 1;
        set_reg(uc, UC_ARM_REG_R0, run->writer_status);
        set_reg(uc, UC_ARM_REG_PC, reg(uc, UC_ARM_REG_LR));
    } else if (pc == HELPER) {
        run->outcome = HELPER_REACHED;
        checked(uc_emu_stop(uc));
    }
}

static void writes(uc_engine *uc, uc_mem_type type, uint64_t address, int size,
                   int64_t value, void *opaque) {
    (void)type;
    (void)value;
    Run *run = opaque;
    if (address == LED_DATA && size == 4) {
        run->led_writes++;
        if (run->stop_led_writes != 0 && run->led_writes >= run->stop_led_writes) {
            run->outcome = REFUSED;
            checked(uc_emu_stop(uc));
        }
    }
}

static uc_engine *machine(const uint8_t *stub, size_t stub_size, Run *run,
                          uc_hook *code_hook, uc_hook *write_hook) {
    uc_engine *uc;
    checked(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    checked(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    checked(uc_mem_map(uc, 0, 0x100000, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0x00600000u, 0x00300000, UC_PROT_ALL));
    checked(uc_mem_map(uc, STACK_BASE, 0x1000, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0xfff00000u, 0x100000, UC_PROT_ALL));
    checked(uc_mem_write(uc, GUARD, stub, stub_size));
    checked(uc_hook_add(uc, code_hook, UC_HOOK_CODE, (void *)code, run, 1, 0));
    checked(uc_hook_add(uc, write_hook, UC_HOOK_MEM_WRITE, (void *)writes, run, 1, 0));
    return uc;
}

static void exercise_guard(const uint8_t *stub, size_t size, uint32_t caller, uint32_t stack,
                           Outcome wanted) {
    Run run = {.outcome = RUNNING, .stop_led_writes = 6};
    uc_hook code_hook, write_hook;
    uc_engine *uc = machine(stub, size, &run, &code_hook, &write_hook);
    checked(uc_mem_write(uc, ROM_STACK - 4, &caller, 4));
    set_reg(uc, UC_ARM_REG_SP, stack);
    checked(uc_emu_start(uc, GUARD, 0x10000, 1000000, 4096));
    if (run.outcome != wanted || run.bad) {
        fprintf(stderr, "guard caller=%08x stack=%08x outcome=%d wanted=%d bad=%d pc=%08x\n",
                caller, stack, run.outcome, wanted, run.bad, reg(uc, UC_ARM_REG_PC));
        exit(EXIT_FAILURE);
    }
    checked(uc_close(uc));
}

static void exercise_callback(const uint8_t *stub, size_t size, uint32_t tap,
                              uint32_t completion, uint32_t discriminator, Outcome wanted) {
    Run run = {.outcome = RUNNING, .stop_led_writes = wanted == REFUSED ? 6u : 0u};
    uc_hook code_hook, write_hook;
    uc_engine *uc = machine(stub, size, &run, &code_hook, &write_hook);
    uint32_t classifier[] = {0xe3a00000u | discriminator, 0xe12fff1eu};
    checked(uc_mem_write(uc, CLASSIFIER, classifier, sizeof classifier));
    checked(uc_mem_write(uc, TAP, &tap, 4));
    checked(uc_mem_write(uc, COMPLETION, &completion, 4));
    set_reg(uc, UC_ARM_REG_SP, STACK_TOP);
    checked(uc_emu_start(uc, CALLBACK, 0x10000, 1000000, 4096));
    uint32_t actual_tap, actual_completion;
    checked(uc_mem_read(uc, TAP, &actual_tap, 4));
    checked(uc_mem_read(uc, COMPLETION, &actual_completion, 4));
    if (run.outcome != wanted || run.bad ||
        (wanted == CLOSE_REACHED && (actual_tap != STOCK || actual_completion != REPAIR_BRANCH))) {
        fprintf(stderr, "callback tap=%08x completion=%08x discriminator=%u outcome=%d wanted=%d actual=%08x/%08x pc=%08x\n",
                tap, completion, discriminator, run.outcome, wanted, actual_tap, actual_completion,
                reg(uc, UC_ARM_REG_PC));
        exit(EXIT_FAILURE);
    }
    checked(uc_close(uc));
}

static void exercise_stage(const uint8_t *stub, size_t size, int ready, int corrupt,
                           uint32_t writer_status, Outcome wanted, unsigned wanted_calls) {
    Run run = {.outcome = RUNNING, .writer_status = writer_status,
               .stop_led_writes = wanted == REFUSED ? 6u : 0u};
    uc_hook code_hook, write_hook;
    uc_engine *uc = machine(stub, size, &run, &code_hook, &write_hook);
    uint8_t *source = malloc(SOURCE_SIZE);
    if (!source) exit(EXIT_FAILURE);
    make_source(source);
    if (corrupt) source[12345] ^= 1;
    checked(uc_mem_write(uc, SOURCE, source, SOURCE_SIZE));
    free(source);
    uint32_t ready_word = (uint32_t)ready;
    uint32_t branch = REPAIR_BRANCH;
    checked(uc_mem_write(uc, READY, &ready_word, 4));
    checked(uc_mem_write(uc, COMPLETION, &branch, 4));
    const int ids[] = {UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
        UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7, UC_ARM_REG_R8,
        UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11, UC_ARM_REG_R12, UC_ARM_REG_LR};
    for (unsigned i = 0; i < sizeof ids / sizeof ids[0]; i++)
        set_reg(uc, ids[i], 0x12340000u + i);
    set_reg(uc, UC_ARM_REG_SP, STACK_TOP);
    checked(uc_emu_start(uc, STAGE, 0x900000, 0, 5000000));
    uint32_t actual_completion;
    checked(uc_mem_read(uc, COMPLETION, &actual_completion, 4));
    if (run.outcome != wanted || run.writer_calls != wanted_calls || run.bad ||
        actual_completion != COMPLETION_WORD) {
        fprintf(stderr, "stage ready=%d corrupt=%d status=%u outcome=%d wanted=%d calls=%u/%u led=%u pc=%08x completion=%08x\n",
                ready, corrupt, writer_status, run.outcome, wanted, run.writer_calls,
                wanted_calls, run.led_writes, reg(uc, UC_ARM_REG_PC), actual_completion);
        exit(EXIT_FAILURE);
    }
    if (wanted == HELPER_REACHED) {
        for (unsigned i = 0; i < sizeof ids / sizeof ids[0]; i++)
            if (reg(uc, ids[i]) != 0x12340000u + i) {
                fprintf(stderr, "stage ABI register %u changed: %08x\n", i, reg(uc, ids[i]));
                exit(EXIT_FAILURE);
            }
        if (reg(uc, UC_ARM_REG_SP) != STACK_TOP || run.led_writes != 8) {
            fprintf(stderr, "stage success stack=%08x led=%u\n", reg(uc, UC_ARM_REG_SP), run.led_writes);
            exit(EXIT_FAILURE);
        }
    }
    checked(uc_close(uc));
}

int main(int argc, char **argv) {
    if (argc != 2) return EXIT_FAILURE;
    FILE *file = fopen(argv[1], "rb");
    if (!file) return EXIT_FAILURE;
    if (fseek(file, 0, SEEK_END) != 0) return EXIT_FAILURE;
    long length = ftell(file);
    if (length <= 0x200 || length > 0x1000 || fseek(file, 0, SEEK_SET) != 0) return EXIT_FAILURE;
    uint8_t *stub = malloc((size_t)length);
    if (!stub || fread(stub, 1, (size_t)length, file) != (size_t)length || fclose(file) != 0) return EXIT_FAILURE;

    exercise_guard(stub, (size_t)length, 0x15cu, ROM_STACK, RESET_REACHED);
    exercise_guard(stub, (size_t)length, 0x174u, ROM_STACK, RESET_REACHED);
    exercise_guard(stub, (size_t)length, 0x144u, ROM_STACK, REFUSED);
    exercise_guard(stub, (size_t)length, 0x15cu, ROM_STACK - 4, REFUSED);
    exercise_callback(stub, (size_t)length, RECORDER, COMPLETION_WORD, 0, CLOSE_REACHED);
    exercise_callback(stub, (size_t)length, STOCK, COMPLETION_WORD, 0, REFUSED);
    exercise_callback(stub, (size_t)length, RECORDER, 0xdeadbeefu, 0, REFUSED);
    exercise_callback(stub, (size_t)length, RECORDER, COMPLETION_WORD, 1, REFUSED);
    exercise_stage(stub, (size_t)length, 1, 0, 0, HELPER_REACHED, 1);
    exercise_stage(stub, (size_t)length, 0, 0, 1, REFUSED, 0);
    exercise_stage(stub, (size_t)length, 1, 1, 1, REFUSED, 0);
    exercise_stage(stub, (size_t)length, 1, 0, 7, REFUSED, 1);
    free(stub);
    puts("PASS persistent repair guard, callback, CRC gate, one-block call and failure paths");
    return EXIT_SUCCESS;
}
