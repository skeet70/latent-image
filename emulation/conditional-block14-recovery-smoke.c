#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/arm.h>
#include <unicorn/unicorn.h>

#define STAGE 0x00006200u
#define READ_DESTINATION 0x00020000u
#define REPAIR_SOURCE 0x00040000u
#define BLOCK_SIZE 0x00020000u
#define READER 0x006a8ff0u
#define LOWER_READER 0x006a8f1cu
#define WRITER 0x006a9268u
#define HELPER 0x00723a14u
#define COMPLETION 0x0073b564u
#define READY 0x00828574u
#define COMPLETION_WORD 0xebffa12au
#define STAGE_BRANCH 0xebe32b25u
#define RC0_CRC 0x92863e8bu
#define RECORDER_CRC 0xd1943bd0u
#define STACK_BASE 0x3b000000u
#define STACK_TOP (STACK_BASE + 0x1000u)
#define LED_DATA 0xfff893c0u

typedef enum { RUNNING, HELPER_REACHED, BLINKING } Outcome;
typedef struct {
    Outcome outcome;
    uint32_t first_reader_status;
    uint32_t second_reader_status;
    uint32_t writer_status;
    uint32_t repair_crc;
    uint32_t medium_crc;
    uint32_t post_write_crc;
    unsigned reader_calls;
    unsigned writer_calls;
    unsigned led_writes;
    unsigned stop_led_writes;
    unsigned sync_delays;
    unsigned long_delays;
    unsigned short_delays;
    unsigned wanted_code;
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

static void make_source(uint8_t *bytes, uint32_t wanted) {
    memset(bytes, 0, BLOCK_SIZE);
    uint32_t base = crc32(bytes, BLOCK_SIZE);
    uint32_t basis[32] = {0};
    uint32_t combinations[32] = {0};
    for (unsigned input = 0; input < 32; input++) {
        bytes[BLOCK_SIZE - 4 + input / 8] ^= (uint8_t)(1u << (input % 8));
        uint32_t value = crc32(bytes, BLOCK_SIZE) ^ base;
        bytes[BLOCK_SIZE - 4 + input / 8] ^= (uint8_t)(1u << (input % 8));
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
            bytes[BLOCK_SIZE - 4 + input / 8] ^= (uint8_t)(1u << (input % 8));
    if (crc32(bytes, BLOCK_SIZE) != wanted) exit(EXIT_FAILURE);
}

static void install_crc_block(uc_engine *uc, uint32_t address, uint32_t crc) {
    uint8_t *bytes = malloc(BLOCK_SIZE);
    if (!bytes) exit(EXIT_FAILURE);
    make_source(bytes, crc);
    checked(uc_mem_write(uc, address, bytes, BLOCK_SIZE));
    free(bytes);
}

static void code(uc_engine *uc, uint64_t pc, uint32_t size, void *opaque) {
    (void)size;
    Run *run = opaque;
    uint32_t word = 0;
    checked(uc_mem_read(uc, pc, &word, sizeof word));
    if (word == 0xee070f3au || word == 0xee070f35u || word == 0xee070f15u) {
        set_reg(uc, UC_ARM_REG_PC, (uint32_t)pc + 4);
    } else if (word == 0xe2500001u) {
#ifdef PACKET_CODED
        uint32_t remaining = reg(uc, UC_ARM_REG_R0);
        if (remaining == 0x08000000u) run->sync_delays++;
        else if (remaining == 0x02000000u) run->long_delays++;
        else if (remaining == 0x00800000u) run->short_delays++;
        else run->bad = 1;
#endif
        set_reg(uc, UC_ARM_REG_R0, 1);
    } else if (pc == LOWER_READER) {
        run->reader_calls++;
        uint32_t fifth;
        checked(uc_mem_read(uc, reg(uc, UC_ARM_REG_SP), &fifth, 4));
        if (reg(uc, UC_ARM_REG_R0) != 14 || reg(uc, UC_ARM_REG_R1) != 1 ||
            reg(uc, UC_ARM_REG_R2) != READ_DESTINATION || reg(uc, UC_ARM_REG_R3) != 0 ||
            fifth != 1 || reg(uc, UC_ARM_REG_SP) % 8 != 0)
            run->bad = 1;
        uint32_t status = run->reader_calls == 1 ? run->first_reader_status : run->second_reader_status;
        if (status == 0) install_crc_block(uc, READ_DESTINATION, run->medium_crc);
        set_reg(uc, UC_ARM_REG_R0, status);
        set_reg(uc, UC_ARM_REG_PC, reg(uc, UC_ARM_REG_LR));
    } else if (pc == WRITER) {
        run->writer_calls++;
        if (reg(uc, UC_ARM_REG_R0) != 14 || reg(uc, UC_ARM_REG_R1) != 1 ||
            reg(uc, UC_ARM_REG_R2) != REPAIR_SOURCE || reg(uc, UC_ARM_REG_SP) % 8 != 0)
            run->bad = 1;
        run->medium_crc = run->post_write_crc;
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
    Run *run = opaque;
    if (address == LED_DATA && size == 4) {
#ifdef PACKET_CODED
        uint32_t expected = run->led_writes % 2 == 0 ? 0x00010001u : 0x00010000u;
        if ((uint32_t)value != expected) run->bad = 1;
#else
        (void)value;
#endif
        run->led_writes++;
        if (run->stop_led_writes != 0 && run->led_writes >= run->stop_led_writes) {
            run->outcome = BLINKING;
            checked(uc_emu_stop(uc));
        }
    }
}

static void exercise(const uint8_t *stub, size_t size, uint32_t ready,
                     uint32_t first_reader_status, uint32_t second_reader_status,
                     uint32_t writer_status, uint32_t repair_crc, uint32_t initial_crc,
                     uint32_t post_write_crc, Outcome wanted, unsigned wanted_reads,
                     unsigned wanted_writes, unsigned wanted_led_writes, unsigned wanted_code) {
    Run run = {.outcome = RUNNING, .first_reader_status = first_reader_status,
               .second_reader_status = second_reader_status, .writer_status = writer_status,
               .repair_crc = repair_crc, .medium_crc = initial_crc,
               .post_write_crc = post_write_crc, .wanted_code = wanted_code,
               .stop_led_writes = wanted == BLINKING ? wanted_led_writes : 0};
    uc_engine *uc;
    checked(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    checked(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    checked(uc_mem_map(uc, 0, 0x100000, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0x00600000u, 0x00300000, UC_PROT_ALL));
    checked(uc_mem_map(uc, STACK_BASE, 0x1000, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0xfff00000u, 0x100000, UC_PROT_ALL));
    checked(uc_mem_write(uc, 0x6000, stub, size));
    const uint32_t reader_adapter[] = {
        0xe92d4800u, 0xe28db004u, 0xe24dd008u, 0xe58d3000u,
        0xe3a03000u, 0xebffffc4u, 0xe24bd004u, 0xe8bd8800u,
    };
    checked(uc_mem_write(uc, READER, reader_adapter, sizeof reader_adapter));
    install_crc_block(uc, REPAIR_SOURCE, repair_crc);
    uint32_t completion = STAGE_BRANCH;
    checked(uc_mem_write(uc, COMPLETION, &completion, 4));
    checked(uc_mem_write(uc, READY, &ready, 4));
    const int ids[] = {UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
        UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7, UC_ARM_REG_R8,
        UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11, UC_ARM_REG_R12, UC_ARM_REG_LR};
    for (unsigned i = 0; i < sizeof ids / sizeof ids[0]; i++)
        set_reg(uc, ids[i], 0x12340000u + i);
    set_reg(uc, UC_ARM_REG_SP, STACK_TOP);
    uc_hook code_hook, write_hook;
    checked(uc_hook_add(uc, &code_hook, UC_HOOK_CODE, (void *)code, &run, 1, 0));
    checked(uc_hook_add(uc, &write_hook, UC_HOOK_MEM_WRITE, (void *)writes, &run, 1, 0));
    checked(uc_emu_start(uc, STAGE, 0x900000, 0, 30000000));
    uint32_t actual_completion;
    checked(uc_mem_read(uc, COMPLETION, &actual_completion, 4));
    if (run.outcome != wanted || run.reader_calls != wanted_reads ||
        run.writer_calls != wanted_writes || run.led_writes != wanted_led_writes ||
        run.bad || actual_completion != COMPLETION_WORD
#ifdef PACKET_CODED
        || run.sync_delays != 2 || run.long_delays != 2 * run.wanted_code - 1 ||
        run.short_delays != 0
#endif
        ) {
        fprintf(stderr, "ready=%u rstatus=%u/%u wstatus=%u repair=%08x initial=%08x post=%08x outcome=%d/%d reads=%u/%u writes=%u/%u led=%u/%u delays=%u/%u/%u code=%u bad=%d completion=%08x\n",
                ready, first_reader_status, second_reader_status, writer_status, repair_crc,
                initial_crc, post_write_crc, run.outcome, wanted, run.reader_calls,
                wanted_reads, run.writer_calls, wanted_writes, run.led_writes,
                wanted_led_writes, run.sync_delays, run.long_delays, run.short_delays,
                run.wanted_code, run.bad, actual_completion);
        exit(EXIT_FAILURE);
    }
    if (wanted == HELPER_REACHED) {
        for (unsigned i = 0; i < sizeof ids / sizeof ids[0]; i++)
            if (reg(uc, ids[i]) != 0x12340000u + i) exit(EXIT_FAILURE);
        if (reg(uc, UC_ARM_REG_SP) != STACK_TOP) exit(EXIT_FAILURE);
    }
    checked(uc_close(uc));
}

int main(int argc, char **argv) {
    if (argc != 2) return EXIT_FAILURE;
    FILE *file = fopen(argv[1], "rb");
    if (!file || fseek(file, 0, SEEK_END) != 0) return EXIT_FAILURE;
    long length = ftell(file);
    if (length <= 0x200 || length > 0x1000 || fseek(file, 0, SEEK_SET) != 0) return EXIT_FAILURE;
    uint8_t *stub = malloc((size_t)length);
    if (!stub || fread(stub, 1, (size_t)length, file) != (size_t)length || fclose(file) != 0)
        return EXIT_FAILURE;
#ifdef PACKET_CODED
    exercise(stub, (size_t)length, 0, 0, 0, 0, RC0_CRC, RECORDER_CRC, RC0_CRC, BLINKING, 0, 0, 4, 1);
    exercise(stub, (size_t)length, 1, 7, 0, 0, RC0_CRC, RECORDER_CRC, RC0_CRC, BLINKING, 1, 0, 6, 2);
    exercise(stub, (size_t)length, 1, 0, 0, 0, RC0_CRC, RC0_CRC, RC0_CRC, BLINKING, 1, 0, 10, 4);
    exercise(stub, (size_t)length, 1, 0, 0, 0, RC0_CRC, 0x12345678u, RC0_CRC, BLINKING, 1, 0, 8, 3);
    exercise(stub, (size_t)length, 1, 0, 0, 0, 0x12345678u, RECORDER_CRC, RC0_CRC, BLINKING, 1, 0, 12, 5);
    exercise(stub, (size_t)length, 1, 0, 0, 0, RC0_CRC, RECORDER_CRC, RC0_CRC, BLINKING, 2, 1, 18, 7);
    exercise(stub, (size_t)length, 1, 0, 0, 7, RC0_CRC, RECORDER_CRC, RC0_CRC, BLINKING, 2, 1, 18, 7);
    exercise(stub, (size_t)length, 1, 0, 0, 7, RC0_CRC, RECORDER_CRC, RECORDER_CRC, BLINKING, 2, 1, 20, 8);
    exercise(stub, (size_t)length, 1, 0, 7, 0, RC0_CRC, RECORDER_CRC, RC0_CRC, BLINKING, 2, 1, 16, 6);
    exercise(stub, (size_t)length, 1, 0, 0, 0, RC0_CRC, RECORDER_CRC, 0x12345678u, BLINKING, 2, 1, 22, 9);
    puts("PASS packet-coded conditional block-14 outcomes 1 through 9");
#else
    exercise(stub, (size_t)length, 0, 0, 0, 0, RC0_CRC, RECORDER_CRC, RC0_CRC, BLINKING, 0, 0, 6, 0);
    exercise(stub, (size_t)length, 1, 7, 0, 0, RC0_CRC, RECORDER_CRC, RC0_CRC, BLINKING, 1, 0, 6, 0);
    exercise(stub, (size_t)length, 1, 0, 0, 0, RC0_CRC, RC0_CRC, RC0_CRC, HELPER_REACHED, 1, 0, 7, 0);
    exercise(stub, (size_t)length, 1, 0, 0, 0, RC0_CRC, 0x12345678u, RC0_CRC, HELPER_REACHED, 1, 0, 5, 0);
    exercise(stub, (size_t)length, 1, 0, 0, 0, 0x12345678u, RECORDER_CRC, RC0_CRC, BLINKING, 1, 0, 6, 0);
    exercise(stub, (size_t)length, 1, 0, 0, 0, RC0_CRC, RECORDER_CRC, RC0_CRC, HELPER_REACHED, 2, 1, 9, 0);
    exercise(stub, (size_t)length, 1, 0, 0, 7, RC0_CRC, RECORDER_CRC, RC0_CRC, HELPER_REACHED, 2, 1, 9, 0);
    exercise(stub, (size_t)length, 1, 0, 0, 7, RC0_CRC, RECORDER_CRC, RECORDER_CRC, HELPER_REACHED, 2, 1, 5, 0);
    exercise(stub, (size_t)length, 1, 0, 7, 0, RC0_CRC, RECORDER_CRC, RC0_CRC, BLINKING, 2, 1, 8, 0);
    exercise(stub, (size_t)length, 1, 0, 0, 0, RC0_CRC, RECORDER_CRC, 0x12345678u, BLINKING, 2, 1, 8, 0);
    puts("PASS conditional block-14 pre-read, bounded write, post-read and visible outcomes");
#endif
    free(stub);
    return EXIT_SUCCESS;
}
