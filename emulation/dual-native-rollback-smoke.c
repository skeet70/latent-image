#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/arm.h>
#include <unicorn/unicorn.h>

#define STUB_SIZE 0x128u
#define GUARD 0x00006000u
#define ROLLBACK 0x000060c0u
#define RESET 0x00001800u
#define CLOSE 0x00002044u
#define TAP 0x0071feb4u
#define STOCK 0xebffffdcu
#define RECORDER 0xeb34a9c9u
#define ROM_STACK 0xfff00f00u
#define LOW_STACK 0x0000f000u
#define EXIT 0x00008000u
#define LED_CONTROL 0xfff88084u
#define LED_DATA 0xfff893c0u
#define LED_SETUP 0xfff893c4u

enum Outcome { RUNNING, RESET_REACHED, CLOSED, REFUSED };

typedef struct {
    enum Outcome outcome;
    unsigned led_control_writes;
    unsigned led_setup_writes;
    unsigned led_data_writes;
    unsigned unexpected_write;
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

static void code(uc_engine *uc, uint64_t pc, uint32_t size, void *opaque) {
    (void)size;
    Run *run = opaque;
    uint32_t word;
    checked(uc_mem_read(uc, pc, &word, sizeof word));
    if (word == 0xe2544001u) {
        uint32_t one = 1;
        checked(uc_reg_write(uc, UC_ARM_REG_R4, &one));
    } else if (word == 0xee070f3au || word == 0xee070f15u) {
        uint32_t next = (uint32_t)pc + 4;
        checked(uc_reg_write(uc, UC_ARM_REG_PC, &next));
    } else if (word == 0xe320f003u) {
        run->outcome = REFUSED;
        checked(uc_emu_stop(uc));
    } else if (pc == RESET) {
        run->outcome = RESET_REACHED;
        checked(uc_emu_stop(uc));
    } else if (pc == CLOSE) {
        const int ids[] = {UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
                           UC_ARM_REG_R3, UC_ARM_REG_R12};
        const unsigned indexes[] = {0, 1, 2, 3, 12};
        for (unsigned i = 0; i < sizeof ids / sizeof ids[0]; i++) {
            if (reg(uc, ids[i]) != 0x12340000u + indexes[i]) {
                fprintf(stderr, "Close ABI changed register %d\n", ids[i]);
                exit(EXIT_FAILURE);
            }
        }
        if (reg(uc, UC_ARM_REG_SP) != LOW_STACK ||
            reg(uc, UC_ARM_REG_LR) != EXIT) {
            fprintf(stderr, "Close ABI changed SP or LR\n");
            exit(EXIT_FAILURE);
        }
    } else if (pc == EXIT) {
        run->outcome = CLOSED;
        checked(uc_emu_stop(uc));
    }
}

static void guard_writes(uc_engine *uc, uc_mem_type type, uint64_t address,
                         int size, int64_t value, void *opaque) {
    (void)uc;
    (void)type;
    Run *run = opaque;
    if (address == LED_CONTROL && size == 4 && (uint32_t)value == 1u) {
        run->led_control_writes++;
        return;
    }
    if (address == LED_SETUP && size == 4 && (uint32_t)value == 0x00050001u) {
        run->led_setup_writes++;
        return;
    }
    if (address == LED_DATA && size == 4) {
        uint32_t expected = run->led_data_writes % 2 ? 0x00010000u : 0x00010001u;
        if ((uint32_t)value != expected) {
            run->unexpected_write = 1;
            checked(uc_emu_stop(uc));
            return;
        }
        run->led_data_writes++;
        return;
    }
    if (address >= ROM_STACK - 44 && address + (unsigned)size <= ROM_STACK) return;
    run->unexpected_write = 1;
    checked(uc_emu_stop(uc));
}

static void rollback_writes(uc_engine *uc, uc_mem_type type, uint64_t address,
                            int size, int64_t value, void *opaque) {
    (void)type;
    (void)value;
    Run *run = opaque;
    if ((address >= LOW_STACK - 24 && address + (unsigned)size <= LOW_STACK) ||
        (address == TAP && size == 4)) return;
    run->unexpected_write = 1;
    checked(uc_emu_stop(uc));
}

static uc_engine *machine(const uint8_t *stub, Run *run, uc_hook *code_hook) {
    uc_engine *uc;
    checked(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    checked(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    checked(uc_mem_map(uc, 0, 0x10000, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0xfff00000u, 0x100000, UC_PROT_ALL));
    checked(uc_mem_write(uc, GUARD, stub, STUB_SIZE));
    checked(uc_hook_add(uc, code_hook, UC_HOOK_CODE, (void *)code, run, 1, 0));
    return uc;
}

static void exercise_guard(const uint8_t *stub, uint32_t caller, uint32_t stack,
                           unsigned expected_pulses) {
    Run run = {.outcome = RUNNING};
    uc_hook code_hook, write_hook;
    uc_engine *uc = machine(stub, &run, &code_hook);
    checked(uc_mem_write(uc, ROM_STACK - 4, &caller, sizeof caller));
    checked(uc_reg_write(uc, UC_ARM_REG_SP, &stack));
    const int preserved[] = {UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
        UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7,
        UC_ARM_REG_R8, UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11,
        UC_ARM_REG_R12, UC_ARM_REG_LR};
    for (unsigned i = 0; i < sizeof preserved / sizeof preserved[0]; i++) {
        uint32_t value = 0x56780000u + i;
        checked(uc_reg_write(uc, preserved[i], &value));
    }
    checked(uc_hook_add(uc, &write_hook, UC_HOOK_MEM_WRITE,
                        (void *)guard_writes, &run, 1, 0));
    checked(uc_emu_start(uc, GUARD, 0x10000, 1000000, 4096));
    enum Outcome expected = expected_pulses ? RESET_REACHED : REFUSED;
    int bad_contract = 0;
    if (expected_pulses) {
        bad_contract = reg(uc, UC_ARM_REG_SP) != ROM_STACK ||
                       reg(uc, UC_ARM_REG_R0) != 0 ||
                       (reg(uc, UC_ARM_REG_CPSR) & 0xf8000000u) != 0;
        for (unsigned i = 0; i < sizeof preserved / sizeof preserved[0]; i++)
            bad_contract |= reg(uc, preserved[i]) != 0x56780000u + i;
    }
    if (run.unexpected_write || run.outcome != expected ||
        run.led_control_writes != (expected_pulses != 0) ||
        run.led_setup_writes != (expected_pulses != 0) ||
        run.led_data_writes != expected_pulses * 2 || bad_contract) {
        fprintf(stderr, "Guard failed: caller=%08x stack=%08x outcome=%d writes=%u\n",
                caller, stack, run.outcome, run.led_data_writes);
        exit(EXIT_FAILURE);
    }
    printf("PASS guard caller=%08x outcome=%s pulses=%u\n", caller,
           expected_pulses ? "reset" : "refused", expected_pulses);
    checked(uc_close(uc));
}

static void exercise_rollback(const uint8_t *stub, uint32_t initial_tap,
                              unsigned discriminator) {
    Run run = {.outcome = RUNNING};
    uc_hook code_hook, write_hook;
    uc_engine *uc = machine(stub, &run, &code_hook);
    uint32_t classifier[] = {0xe3a00000u | discriminator, 0xe12fff1eu};
    uint32_t close[] = {0xe3a00007u, 0xe12fff1eu};
    checked(uc_mem_write(uc, 0x1aa0, classifier, sizeof classifier));
    checked(uc_mem_write(uc, CLOSE, close, sizeof close));
    uint8_t expected[4096], observed[4096];
    memset(expected, 0xa5, sizeof expected);
    memcpy(expected + (TAP & 4095), &initial_tap, sizeof initial_tap);
    checked(uc_mem_map(uc, TAP & ~4095u, 4096, UC_PROT_READ | UC_PROT_WRITE));
    checked(uc_mem_write(uc, TAP & ~4095u, expected, sizeof expected));
    const int ids[] = {UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
        UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7, UC_ARM_REG_R8,
        UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11, UC_ARM_REG_R12};
    for (unsigned i = 0; i < sizeof ids / sizeof ids[0]; i++) {
        uint32_t value = 0x12340000u + i;
        checked(uc_reg_write(uc, ids[i], &value));
    }
    uint32_t sp = LOW_STACK, lr = EXIT;
    checked(uc_reg_write(uc, UC_ARM_REG_SP, &sp));
    checked(uc_reg_write(uc, UC_ARM_REG_LR, &lr));
    checked(uc_hook_add(uc, &write_hook, UC_HOOK_MEM_WRITE,
                        (void *)rollback_writes, &run, 1, 0));
    checked(uc_emu_start(uc, ROLLBACK, EXIT + 4, 1000000, 256));
    int accepted = discriminator == 0 && (initial_tap == STOCK || initial_tap == RECORDER);
    if (accepted) memcpy(expected + (TAP & 4095), &(uint32_t){STOCK}, 4);
    checked(uc_mem_read(uc, TAP & ~4095u, observed, sizeof observed));
    if (run.unexpected_write || run.outcome != (accepted ? CLOSED : REFUSED) ||
        memcmp(expected, observed, sizeof expected) != 0) {
        fprintf(stderr, "Rollback failed: tap=%08x discriminator=%u outcome=%d\n",
                initial_tap, discriminator, run.outcome);
        exit(EXIT_FAILURE);
    }
    if (accepted && (reg(uc, UC_ARM_REG_SP) != LOW_STACK || reg(uc, UC_ARM_REG_LR) != EXIT))
        exit(EXIT_FAILURE);
    printf("PASS rollback tap=%08x discriminator=%u outcome=%s\n", initial_tap,
           discriminator, accepted ? "closed" : "refused");
    checked(uc_close(uc));
}

int main(int argc, char **argv) {
    if (argc != 2) return EXIT_FAILURE;
    FILE *file = fopen(argv[1], "rb");
    if (!file) { perror(argv[1]); return EXIT_FAILURE; }
    uint8_t stub[STUB_SIZE];
    size_t count = fread(stub, 1, sizeof stub, file);
    int extra = fgetc(file);
    int bad = ferror(file);
    if (fclose(file) != 0 || bad || count != sizeof stub || extra != EOF)
        return EXIT_FAILURE;
    exercise_guard(stub, 0x0000015cu, ROM_STACK, 1);
    exercise_guard(stub, 0x8000015cu, ROM_STACK, 1);
    exercise_guard(stub, 0x00000174u, ROM_STACK, 2);
    exercise_guard(stub, 0x80000174u, ROM_STACK, 2);
    exercise_guard(stub, 0x00000144u, ROM_STACK, 0);
    exercise_guard(stub, 0x0000015cu, ROM_STACK - 4, 0);
    exercise_rollback(stub, RECORDER, 0);
    exercise_rollback(stub, STOCK, 0);
    exercise_rollback(stub, 0xdeadbeefu, 0);
    exercise_rollback(stub, RECORDER, 1);
    return EXIT_SUCCESS;
}
