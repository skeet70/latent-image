#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/arm.h>
#include <unicorn/unicorn.h>

#define CALLBACK 0x000060c0u
#define STAGE 0x00006200u
#define SNAPSHOT 0x00006f00u
#define BOOT_LIST 0x00004310u
#define READ_DESTINATION 0x00020000u
#define REPAIR_SOURCE READ_DESTINATION
#define RC0_REFERENCE 0x00008000u
#define BLOCK_SIZE 0x00020000u
#define PRIMARY_DISCRIMINATOR 0x00001aa0u
#define NATIVE_CLOSE 0x00002044u
#define CACHE_INVALIDATE 0x00042634u
#define STORAGE_LOCK 0x006faf00u
#define STORAGE_UNLOCK 0x006faf40u
#define PHYSICAL_READER 0x006fc138u
#define UNIT_READER 0x006f1440u
#define BLOCK_PROGRAM 0x006faf5cu
#define MAP_REFRESH 0x006a90acu
#define SOURCE_CLEAN 0x0004263cu
#define LOWER_LOCK 0x006fbac4u
#define LOWER_UNLOCK 0x006fbadcu
#define TAP 0x0071feb4u
#define COMPLETION 0x0073b564u
#define READY 0x00828574u
#define APP_REMAP_14 0x0082859cu
#define APP_BAD_LIST_PTR 0x009552f0u
#define UNITS_PER_BLOCK 0x0082984eu
#define DEVICE_INDEX 0x00918844u
#define LED_DATA 0xfff893c0u
#define RECORDER_TAP 0xeb34a9c9u
#define STOCK_TAP 0xebffffdcu
#define COMPLETION_WORD 0xebffa12au
#define STAGE_BRANCH 0xebe32b25u
#define RC0_CRC 0x92863e8bu
#define RECORDER_CRC 0xd1943bd0u
#define STACK_BASE 0x3b000000u
#define STACK_TOP (STACK_BASE + 0x1000u)

typedef enum { RUNNING, CALLBACK_DONE, PACKETS_REACHED } Outcome;
typedef enum { READ_INTACT, READ_CHANGED_BODY, READ_CHANGED_PATCH_WORD } ReadFault;
typedef struct {
    Outcome outcome;
    uint32_t lock_status;
    uint32_t app_lock_status;
    uint32_t raw_status;
    uint32_t app_raw_status;
    uint32_t final_raw_status;
    uint32_t post_raw_status;
    uint32_t writer_status;
    uint32_t raw_crc;
    uint32_t app_raw_crc;
    uint32_t post_raw_crc;
    uint32_t source_crc;
    uint32_t expected_raw_physical;
    uint32_t expected_app_physical;
    uint32_t expected_post_physical;
    uint32_t post_app_physical;
    unsigned raw_reads;
    unsigned writer_calls;
    unsigned packet_start;
    int repair;
    int change_before_write;
    int invalid_post_map;
    unsigned locks;
    unsigned unlocks;
    unsigned cache_invalidates;
    unsigned lower_locks;
    unsigned lower_unlocks;
    unsigned led_writes;
    unsigned stop_led_writes;
    unsigned long_delays;
    unsigned delay_at_led[128];
    int bad;
    uint8_t *captured_block;
    int preserve_carried_buffers;
    ReadFault initial_read_fault;
    ReadFault final_read_fault;
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

static uint32_t word_at(uc_engine *uc, uint32_t address) {
    uint32_t value;
    checked(uc_mem_read(uc, address, &value, sizeof value));
    return value;
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
    for (size_t i = 0; i < BLOCK_SIZE; i++)
        bytes[i] = (uint8_t)((i * 73) ^ (i >> 7) ^ (i >> 15) ^ 0xa5);
    if (wanted == RC0_CRC || wanted == RECORDER_CRC) {
        const uint32_t offsets[] = {0x1f120, 0x1f124, 0x1f480, 0x1f484};
        const uint32_t rc0[] = {0xffdc0446, 0x04d8ebff, 0x28024381, 0x0803c8da};
        const uint32_t recorder[] = {0xa9c90446, 0x04d8eb34, 0x280d4381, 0xebffffda};
        for (size_t i = 0; i < 4; i++)
            memcpy(bytes + offsets[i], wanted == RC0_CRC ? rc0 + i : recorder + i, 4);
    }
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

typedef struct {
    uint32_t address;
    size_t offset;
    size_t length;
} ReferenceSpan;

static const ReferenceSpan reference_spans[] = {
    {0x8000, 0, 0x8000},
    {0x16000, 0x8000, 0x8000},
    {0x88000, 0x10000, 0x10000},
};

static int reference_matches_body(const uint8_t *bytes, const uint8_t *body) {
    for (size_t i = 0; i < sizeof reference_spans / sizeof reference_spans[0]; i++) {
        ReferenceSpan span = reference_spans[i];
        if (memcmp(bytes + span.offset, body + span.address, span.length) != 0)
            return 0;
    }
    return 1;
}

static void install_crc_block(uc_engine *uc, uint32_t destination, uint32_t crc) {
    uint8_t *bytes = malloc(BLOCK_SIZE);
    if (!bytes) exit(EXIT_FAILURE);
    make_source(bytes, crc);
    if (destination == RC0_REFERENCE) {
        for (size_t i = 0; i < sizeof reference_spans / sizeof reference_spans[0]; i++) {
            ReferenceSpan span = reference_spans[i];
            checked(uc_mem_write(uc, span.address, bytes + span.offset, span.length));
        }
    } else {
        checked(uc_mem_write(uc, destination, bytes, BLOCK_SIZE));
    }
    free(bytes);
}

static void return_from_call(uc_engine *uc, uint32_t value) {
    set_reg(uc, UC_ARM_REG_R0, value);
    set_reg(uc, UC_ARM_REG_PC, reg(uc, UC_ARM_REG_LR));
}

static void code(uc_engine *uc, uint64_t pc, uint32_t size, void *opaque) {
    (void)size;
    Run *run = opaque;
    uint32_t word = 0;
    checked(uc_mem_read(uc, pc, &word, sizeof word));
    if (word == 0xee070f3au || word == 0xee070f35u || word == 0xee070f15u) {
        set_reg(uc, UC_ARM_REG_PC, (uint32_t)pc + 4);
    } else if (word == 0xe2500001u) {
        if (reg(uc, UC_ARM_REG_R0) == 0x02000000u) run->long_delays++;
        set_reg(uc, UC_ARM_REG_R0, 1);
    } else if (pc == PRIMARY_DISCRIMINATOR || pc == 0x427b8) {
        return_from_call(uc, 0);
    } else if (pc == NATIVE_CLOSE) {
        run->outcome = CALLBACK_DONE;
        checked(uc_emu_stop(uc));
    } else if (pc == STORAGE_LOCK) {
        uint32_t request[2];
        uint32_t status = run->locks == 0 ? run->lock_status : run->app_lock_status;
        checked(uc_mem_read(uc, reg(uc, UC_ARM_REG_R0), request, sizeof request));
        if (request[0] != 0 || request[1] != 1000 || reg(uc, UC_ARM_REG_SP) % 8 != 0)
            run->bad = 1;
        run->locks++;
        return_from_call(uc, status);
    } else if (pc == CACHE_INVALIDATE) {
        if (reg(uc, UC_ARM_REG_R0) != 0 || reg(uc, UC_ARM_REG_SP) % 8 != 0)
            run->bad = 1;
        run->cache_invalidates++;
        return_from_call(uc, 0);
    } else if (pc == LOWER_LOCK) {
        if (reg(uc, UC_ARM_REG_SP) % 8 != 0) run->bad = 1;
        run->lower_locks++;
        return_from_call(uc, 0);
    } else if (pc == LOWER_UNLOCK) {
        if (reg(uc, UC_ARM_REG_SP) % 8 != 0) run->bad = 1;
        run->lower_unlocks++;
        return_from_call(uc, 0);
    } else if (pc == UNIT_READER) {
        uint32_t extra[4];
        uint32_t progress[2];
        unsigned read = run->raw_reads;
        uint32_t expected_physical = read == 0 ? run->expected_raw_physical :
                                     read == 1 ? run->expected_app_physical :
                                     read == 2 ? 14 :
                                                 run->expected_post_physical;
        uint32_t status = read == 0 ? run->raw_status :
                          read == 1 ? run->app_raw_status :
                          read == 2 ? run->final_raw_status : run->post_raw_status;
        uint32_t crc = read == 0 ? run->raw_crc :
                       read == 1 ? run->app_raw_crc :
                       read == 2 ? RECORDER_CRC : run->post_raw_crc;
        checked(uc_mem_read(uc, reg(uc, UC_ARM_REG_SP), extra, sizeof extra));
        checked(uc_mem_read(uc, extra[3], progress, sizeof progress));
        if (read >= 4 || reg(uc, UC_ARM_REG_R0) != expected_physical * 64 ||
            reg(uc, UC_ARM_REG_R1) != 64 ||
            reg(uc, UC_ARM_REG_R2) != (READ_DESTINATION | 0x40000000u) ||
            reg(uc, UC_ARM_REG_R3) != 0 || extra[0] != 0 || extra[1] != 1000 ||
            extra[2] != 0 || progress[0] != 0 || progress[1] != 0 ||
            reg(uc, UC_ARM_REG_SP) % 8 != 0)
            run->bad = 1;
        run->raw_reads++;
        if (status == 0) {
            if (run->captured_block != NULL)
                checked(uc_mem_write(uc, READ_DESTINATION, run->captured_block, BLOCK_SIZE));
            else
                install_crc_block(uc, READ_DESTINATION, crc);
        }
        ReadFault fault = read == 0 ? run->initial_read_fault :
                          read == 2 ? run->final_read_fault : READ_INTACT;
        if (status == 0 && fault != READ_INTACT) {
            uint32_t offset = fault == READ_CHANGED_BODY ? 0 : 0x1f121;
            uint8_t byte;
            checked(uc_mem_read(uc, READ_DESTINATION + offset, &byte, 1));
            byte ^= 1;
            checked(uc_mem_write(uc, READ_DESTINATION + offset, &byte, 1));
        }
        if (read == 1 && run->change_before_write) {
            uint16_t changed = 15;
            checked(uc_mem_write(uc, APP_REMAP_14, &changed, sizeof changed));
        }
        return_from_call(uc, status);
    } else if (pc == BLOCK_PROGRAM) {
        uint32_t descriptor = reg(uc, UC_ARM_REG_R0);
        if (word_at(uc, descriptor) != 14 || word_at(uc, descriptor + 4) != 1 ||
            word_at(uc, descriptor + 12) != (REPAIR_SOURCE | 0x40000000u) ||
            word_at(uc, descriptor + 16) != 1 ||
            reg(uc, UC_ARM_REG_SP) % 8 != 0)
            run->bad = 1;
        run->writer_calls++;
        if (run->captured_block != NULL)
            checked(uc_mem_read(uc, REPAIR_SOURCE, run->captured_block, BLOCK_SIZE));
        if (run->invalid_post_map) {
            uint32_t duplicate[] = {2, 14, 14};
            checked(uc_mem_write(uc, BOOT_LIST, duplicate, sizeof duplicate));
        } else if (run->post_app_physical == 15) {
            uint32_t remapped[] = {1, 14};
            checked(uc_mem_write(uc, BOOT_LIST, remapped, sizeof remapped));
        } else {
            uint32_t empty = 0;
            checked(uc_mem_write(uc, BOOT_LIST, &empty, sizeof empty));
        }
        uint16_t mapped = (uint16_t)run->post_app_physical;
        checked(uc_mem_write(uc, APP_REMAP_14, &mapped, sizeof mapped));
        return_from_call(uc, run->writer_status);
    } else if (pc == MAP_REFRESH) {
        return_from_call(uc, 0);
    } else if (pc == SOURCE_CLEAN) {
        if (reg(uc, UC_ARM_REG_R0) != REPAIR_SOURCE ||
            reg(uc, UC_ARM_REG_R1) != BLOCK_SIZE ||
            reg(uc, UC_ARM_REG_SP) % 8 != 0)
            run->bad = 1;
        return_from_call(uc, 0);
    } else if (pc == STORAGE_UNLOCK) {
        run->unlocks++;
        return_from_call(uc, 0);
    }
}

static void writes(uc_engine *uc, uc_mem_type type, uint64_t address, int size,
                   int64_t value, void *opaque) {
    (void)type;
    (void)uc;
    Run *run = opaque;
    if (address == LED_DATA && size == 4) {
        uint32_t expected = run->led_writes % 2 == 0 ? 0x00010001u : 0x00010000u;
        if ((uint32_t)value != expected) run->bad = 1;
        if (run->led_writes >= sizeof run->delay_at_led / sizeof run->delay_at_led[0])
            run->bad = 1;
        else
            run->delay_at_led[run->led_writes] = run->long_delays;
        run->led_writes++;
        if (run->led_writes >= run->stop_led_writes) {
            run->outcome = PACKETS_REACHED;
            checked(uc_emu_stop(uc));
        }
    }
}

static _Alignas(4096) uint8_t low_ram[0x100000];

static uc_engine *machine(const uint8_t *stub, size_t size, Run *run) {
    uc_engine *uc;
    checked(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    checked(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    memset(low_ram, 0, sizeof low_ram);
    checked(uc_mem_map_ptr(uc, 0, sizeof low_ram, UC_PROT_ALL, low_ram));
    checked(uc_mem_map_ptr(uc, 0x40000000, sizeof low_ram, UC_PROT_ALL, low_ram));
    checked(uc_mem_map(uc, 0x00600000u, 0x00400000, UC_PROT_ALL));
    checked(uc_mem_map(uc, STACK_BASE, 0x1000, UC_PROT_ALL));
    checked(uc_mem_map(uc, 0xfff00000u, 0x100000, UC_PROT_ALL));
    checked(uc_mem_write(uc, 0x6000, stub, size));
    const uint32_t physical_adapter[] = {
        0xe92d4800u, 0xe28db004u, 0xe24dd010u, 0xe1a0c001u,
        0xe3091844u, 0xe3401082u, 0xe308e844u, 0xe340e091u,
        0xe59ee000u, 0xe08ee08eu, 0xe081118eu, 0xe5d1100au,
        0xe58d2000u, 0xe58d3004u, 0xe59b3004u, 0xe58d3008u,
        0xe59b3008u, 0xe58d300cu, 0xe0000091u, 0xe1a0200cu,
        0xe3a03000u, 0xebffd4abu, 0xe24bd004u, 0xe8bd8800u,
    };
    checked(uc_mem_write(uc, PHYSICAL_READER, physical_adapter, sizeof physical_adapter));
    const uint8_t units = 64;
    const uint32_t device = 0;
    checked(uc_mem_write(uc, UNITS_PER_BLOCK, &units, sizeof units));
    checked(uc_mem_write(uc, DEVICE_INDEX, &device, sizeof device));
    uc_hook code_hook, write_hook;
    checked(uc_hook_add(uc, &code_hook, UC_HOOK_CODE, (void *)code, run, 1, 0));
    checked(uc_hook_add(uc, &write_hook, UC_HOOK_MEM_WRITE, (void *)writes, run, 1, 0));
    return uc;
}

static void put_word(uc_engine *uc, uint32_t address, uint32_t value) {
    checked(uc_mem_write(uc, address, &value, sizeof value));
}

static void put_list(uc_engine *uc, const uint32_t *entries, unsigned count) {
    put_word(uc, BOOT_LIST, count);
    if (count != 0) checked(uc_mem_write(uc, BOOT_LIST + 4, entries, count * sizeof *entries));
}

static void run_callback(uc_engine *uc, Run *run, const uint32_t *entries, unsigned count,
                         uint32_t wanted_status, uint32_t wanted_physical) {
    put_list(uc, entries, count);
    put_word(uc, TAP, RECORDER_TAP);
    put_word(uc, COMPLETION, COMPLETION_WORD);
    set_reg(uc, UC_ARM_REG_SP, STACK_TOP);
    checked(uc_emu_start(uc, CALLBACK, 0x900000, 0, 1000000));
    uint32_t snapshot[3], tap, completion;
    checked(uc_mem_read(uc, SNAPSHOT, snapshot, sizeof snapshot));
    checked(uc_mem_read(uc, TAP, &tap, sizeof tap));
    checked(uc_mem_read(uc, COMPLETION, &completion, sizeof completion));
    if (run->outcome != CALLBACK_DONE || snapshot[0] != wanted_status ||
        snapshot[1] != wanted_physical || snapshot[2] != count ||
        tap != STOCK_TAP || completion != STAGE_BRANCH || run->bad) {
        fprintf(stderr, "callback outcome=%d status=%u/%u physical=%u/%u count=%u/%u tap=%08x completion=%08x bad=%d\n",
                run->outcome, snapshot[0], wanted_status, snapshot[1], wanted_physical,
                snapshot[2], count, tap, completion, run->bad);
        exit(EXIT_FAILURE);
    }
}

static unsigned packet_end(const Run *run, unsigned start, unsigned sync, unsigned count) {
    unsigned base = run->delay_at_led[start];
    if (run->delay_at_led[start + 1] != base + sync) exit(EXIT_FAILURE);
    for (unsigned pulse = 0; pulse < count; pulse++) {
        unsigned on = start + 2 + 2 * pulse;
        if (run->delay_at_led[on] != base + sync + 4 + 2 * pulse ||
            run->delay_at_led[on + 1] != base + sync + 5 + 2 * pulse)
            exit(EXIT_FAILURE);
    }
    unsigned next = start + 2 + 2 * count;
    if (run->delay_at_led[next] != base + sync + 8 + 2 * count)
        exit(EXIT_FAILURE);
    return next;
}

static void run_stage(uc_engine *uc, Run *run, const uint32_t *entries, unsigned count,
                      uint32_t app_physical, unsigned wanted_status, unsigned wanted_boot_field,
                      unsigned wanted_delta_field) {
    put_list(uc, entries, count);
    put_word(uc, READY, 1);
    put_word(uc, APP_BAD_LIST_PTR, BOOT_LIST);
    uint16_t mapped = (uint16_t)app_physical;
    checked(uc_mem_write(uc, APP_REMAP_14, &mapped, sizeof mapped));
    put_word(uc, COMPLETION, STAGE_BRANCH);
    if (run->repair && !run->preserve_carried_buffers)
        install_crc_block(uc, RC0_REFERENCE, run->source_crc);
    run->outcome = RUNNING;
    run->packet_start = run->repair && wanted_status >= 16 ? 2 : 0;
    unsigned max_status = run->repair ? 29 : 14;
    run->stop_led_writes = run->packet_start + 3 + 2 * wanted_status;
    if (wanted_status >= 8 && wanted_status <= max_status)
        run->stop_led_writes += 2 + 2 * wanted_boot_field + 2 + 2 * wanted_delta_field;
    run->led_writes = 0;
    run->long_delays = 0;
    memset(run->delay_at_led, 0, sizeof run->delay_at_led);
    set_reg(uc, UC_ARM_REG_SP, STACK_TOP);
    checked(uc_emu_start(uc, STAGE, 0x900000, 0, 50000000));
    uint32_t completion;
    checked(uc_mem_read(uc, COMPLETION, &completion, sizeof completion));
    if (run->outcome != PACKETS_REACHED || run->bad || completion != COMPLETION_WORD) {
        fprintf(stderr, "stage outcome=%d status=%u led=%u delays=%u raw=%u locks=%u/%u cache=%u completion=%08x bad=%d\n",
                run->outcome, wanted_status, run->led_writes, run->long_delays,
                run->raw_reads, run->locks, run->unlocks, run->cache_invalidates,
                completion, run->bad);
        exit(EXIT_FAILURE);
    }
    unsigned next = packet_end(run, run->packet_start, 4, wanted_status);
    max_status = run->repair ? 29 : 14;
    if (wanted_status >= 8 && wanted_status <= max_status) {
        next = packet_end(run, next, 8, wanted_boot_field);
        (void)packet_end(run, next, 12, wanted_delta_field);
    }
}

static void physical_pair_case(const uint8_t *stub, size_t size,
                               const uint32_t *boot_entries, unsigned boot_count,
                               const uint32_t *app_entries, unsigned app_count,
                               uint32_t boot_physical, uint32_t app_physical,
                               uint32_t app_crc, unsigned status,
                               unsigned boot_field, unsigned delta_field) {
    Run run = {.raw_crc = RECORDER_CRC, .app_raw_crc = app_crc,
               .expected_raw_physical = boot_physical,
               .expected_app_physical = app_physical};
    uc_engine *uc = machine(stub, size, &run);
    run_callback(uc, &run, boot_entries, boot_count, 0, boot_physical);
    run_stage(uc, &run, app_entries, app_count, app_physical,
              status, boot_field, delta_field);
    if (run.raw_reads != 2 || run.locks != 2 || run.unlocks != 2 ||
        run.lower_locks != 2 || run.lower_unlocks != 2 ||
        run.cache_invalidates != 2)
        exit(EXIT_FAILURE);
    checked(uc_close(uc));
}

static void repair_case(const uint8_t *stub, size_t size, Run run,
                        unsigned status, unsigned boot_field, unsigned delta_field,
                        unsigned expected_reads, unsigned expected_writes) {
    run.repair = 1;
    run.raw_crc = RECORDER_CRC;
    run.app_raw_crc = RECORDER_CRC;
    run.expected_raw_physical = 14;
    run.expected_app_physical = 14;
    uc_engine *uc = machine(stub, size, &run);
    run_callback(uc, &run, NULL, 0, 0, 14);
    run_stage(uc, &run, NULL, 0, 14, status, boot_field, delta_field);
    if (run.raw_reads != expected_reads || run.writer_calls != expected_writes ||
        run.lower_locks != expected_reads || run.lower_unlocks != expected_reads ||
        run.cache_invalidates != expected_reads)
        exit(EXIT_FAILURE);
    checked(uc_close(uc));
}

static void equal_non14_refusal_case(const uint8_t *stub, size_t size) {
    Run run = {.repair = 1, .raw_crc = RECORDER_CRC, .app_raw_crc = RECORDER_CRC,
               .source_crc = RC0_CRC, .expected_raw_physical = 15,
               .expected_app_physical = 15};
    uint32_t one_bad[] = {14};
    uc_engine *uc = machine(stub, size, &run);
    run_callback(uc, &run, one_bad, 1, 0, 15);
    run_stage(uc, &run, one_bad, 1, 15, 13, 2, 1);
    if (run.raw_reads != 2 || run.writer_calls != 0) exit(EXIT_FAILURE);
    checked(uc_close(uc));
}

static void invalid_boot_case(const uint8_t *stub, size_t size) {
    Run run = {0};
    uc_engine *uc = machine(stub, size, &run);
    uint32_t bad[] = {14, 14};
    run_callback(uc, &run, bad, 2, 1, 0);
    run_stage(uc, &run, NULL, 0, 14, 1, 0, 0);
    if (run.raw_reads != 0) exit(EXIT_FAILURE);
    checked(uc_close(uc));
}

static void raw_rc0_case(const uint8_t *stub, size_t size) {
    Run run = {.raw_crc = RC0_CRC, .expected_raw_physical = 14};
    uc_engine *uc = machine(stub, size, &run);
    run_callback(uc, &run, NULL, 0, 0, 14);
    run_stage(uc, &run, NULL, 0, 14, 7, 0, 0);
    if (run.raw_reads != 1 || run.locks != 1 || run.unlocks != 1 ||
        run.lower_locks != 1 || run.lower_unlocks != 1)
        exit(EXIT_FAILURE);
    checked(uc_close(uc));
}

static void refusal_cases(const uint8_t *stub, size_t size) {
    Run lock = {.lock_status = 7, .expected_raw_physical = 14};
    uc_engine *uc = machine(stub, size, &lock);
    run_callback(uc, &lock, NULL, 0, 0, 14);
    run_stage(uc, &lock, NULL, 0, 14, 5, 0, 0);
    if (lock.raw_reads != 0 || lock.unlocks != 0 || lock.cache_invalidates != 0 ||
        lock.lower_locks != 0 || lock.lower_unlocks != 0)
        exit(EXIT_FAILURE);
    checked(uc_close(uc));

    Run raw = {.raw_status = 7, .expected_raw_physical = 14};
    uc = machine(stub, size, &raw);
    run_callback(uc, &raw, NULL, 0, 0, 14);
    run_stage(uc, &raw, NULL, 0, 14, 5, 0, 0);
    if (raw.raw_reads != 1 || raw.locks != 1 || raw.unlocks != 1 ||
        raw.lower_locks != 1 || raw.lower_unlocks != 1)
        exit(EXIT_FAILURE);
    checked(uc_close(uc));

    Run app_lock = {.raw_crc = RECORDER_CRC, .app_lock_status = 7,
                    .expected_raw_physical = 14, .expected_app_physical = 15};
    uc = machine(stub, size, &app_lock);
    run_callback(uc, &app_lock, NULL, 0, 0, 14);
    uint32_t one_bad[] = {14};
    run_stage(uc, &app_lock, one_bad, 1, 15, 8, 1, 2);
    if (app_lock.raw_reads != 1 || app_lock.locks != 2 || app_lock.unlocks != 1 ||
        app_lock.lower_locks != 1 || app_lock.lower_unlocks != 1)
        exit(EXIT_FAILURE);
    checked(uc_close(uc));

    Run app_raw = {.raw_crc = RECORDER_CRC, .app_raw_status = 7,
                   .expected_raw_physical = 14, .expected_app_physical = 15};
    uc = machine(stub, size, &app_raw);
    run_callback(uc, &app_raw, NULL, 0, 0, 14);
    run_stage(uc, &app_raw, one_bad, 1, 15, 8, 1, 2);
    if (app_raw.raw_reads != 2 || app_raw.locks != 2 || app_raw.unlocks != 2 ||
        app_raw.lower_locks != 2 || app_raw.lower_unlocks != 2)
        exit(EXIT_FAILURE);
    checked(uc_close(uc));

    Run remap = {0};
    uc = machine(stub, size, &remap);
    run_callback(uc, &remap, NULL, 0, 0, 14);
    run_stage(uc, &remap, NULL, 0, 15, 4, 0, 0);
    if (remap.raw_reads != 0) exit(EXIT_FAILURE);
    checked(uc_close(uc));

    Run list = {0};
    uc = machine(stub, size, &list);
    run_callback(uc, &list, NULL, 0, 0, 14);
    uint32_t duplicate[] = {14, 14};
    run_stage(uc, &list, duplicate, 2, 14, 3, 0, 0);
    if (list.raw_reads != 0) exit(EXIT_FAILURE);
    checked(uc_close(uc));
}

static uint8_t *read_exact_file(const char *path, size_t length) {
    FILE *file = fopen(path, "rb");
    uint8_t *bytes = malloc(length);
    if (!file || !bytes || fread(bytes, 1, length, file) != length ||
        fgetc(file) != EOF || fclose(file) != 0)
        exit(EXIT_FAILURE);
    return bytes;
}

static void captured_startup_recovery(const uint8_t *stub, size_t size,
                                    const char *body_path, const char *kernel_path,
                                    const char *recorder_path) {
    uint8_t *body = read_exact_file(body_path, 0xa0000);
    uint8_t *kernel = read_exact_file(kernel_path, 0xdae0);
    uint8_t *media = read_exact_file(recorder_path, BLOCK_SIZE);
    if (memcmp(body + 0x6000, stub, size) != 0) exit(EXIT_FAILURE);
    Run run = {.repair = 1, .preserve_carried_buffers = 1,
               .captured_block = media,
               .expected_raw_physical = 14, .expected_app_physical = 14,
               .expected_post_physical = 14, .post_app_physical = 14};
    uc_engine *uc = machine(stub, size, &run);
    checked(uc_mem_write(uc, 0, body, 0xa0000));
    checked(uc_mem_write(uc, 0x40000, kernel, 0xdae0));
    run_callback(uc, &run, NULL, 0, 0, 14);
    set_reg(uc, UC_ARM_REG_SP, STACK_TOP);
    set_reg(uc, UC_ARM_REG_LR, 0x10000);
    set_reg(uc, UC_ARM_REG_R0, 0x00062c54);
    set_reg(uc, UC_ARM_REG_R1, 0x80c);
    checked(uc_emu_start(uc, 0x434f0, 0x10000, 0, 1000000));
    set_reg(uc, UC_ARM_REG_R0, 0x0004dae0);
    set_reg(uc, UC_ARM_REG_R1, 0x15174);
    checked(uc_emu_start(uc, 0x434f0, 0x10000, 0, 1000000));
    set_reg(uc, UC_ARM_REG_R0, 0x400a0800);
    set_reg(uc, UC_ARM_REG_R1, 0x20100);
    checked(uc_emu_start(uc, 0x434f0, 0x10000, 0, 1000000));
    set_reg(uc, UC_ARM_REG_SP, 0x63460);
    set_reg(uc, UC_ARM_REG_LR, 0x10000);
    set_reg(uc, UC_ARM_REG_R0, 0);
    checked(uc_emu_start(uc, 0x41354, 0x10000, 0, 1000000));
    set_reg(uc, UC_ARM_REG_CPSR, 0xd3);
    set_reg(uc, UC_ARM_REG_SP, 0xa0000);
    set_reg(uc, UC_ARM_REG_CPSR, 0xd2);
    set_reg(uc, UC_ARM_REG_SP, 0x70002000);
    set_reg(uc, UC_ARM_REG_LR, 0x43708);
    set_reg(uc, UC_ARM_REG_SPSR, 0x93);
    checked(uc_emu_start(uc, 0x40204, 0x40234, 0, 1000000));
    uint8_t reference[BLOCK_SIZE];
    for (size_t i = 0; i < sizeof reference_spans / sizeof reference_spans[0]; i++) {
        ReferenceSpan span = reference_spans[i];
        checked(uc_mem_read(uc, span.address, reference + span.offset, span.length));
    }
    if (!reference_matches_body(reference, body)) {
        fputs("RC0 reference changed after captured startup/log initialization/idle IRQ\n", stderr);
        exit(EXIT_FAILURE);
    }
    puts("PASS carried RC0 reference survives captured startup, log initialization and idle IRQ");
    run_stage(uc, &run, NULL, 0, 14, 20, 1, 1);
    if (run.raw_reads != 4 || run.writer_calls != 1 ||
        !reference_matches_body(media, body))
        exit(EXIT_FAILURE);
    puts("PASS captured startup reaches exact RC0 physical readback, status 20");
    checked(uc_close(uc));
    free(kernel);
    free(media);
    free(body);
}

int main(int argc, char **argv) {
    if (argc != 2 && argc != 3 && argc != 5) return EXIT_FAILURE;
    FILE *file = fopen(argv[1], "rb");
    if (!file || fseek(file, 0, SEEK_END) != 0) return EXIT_FAILURE;
    long length = ftell(file);
    if (length <= 0x200 || length > 0x1000 || fseek(file, 0, SEEK_SET) != 0) return EXIT_FAILURE;
    uint8_t *stub = malloc((size_t)length);
    if (!stub || fread(stub, 1, (size_t)length, file) != (size_t)length || fclose(file) != 0)
        return EXIT_FAILURE;
    if (argc == 5) {
        captured_startup_recovery(stub, (size_t)length, argv[2], argv[3], argv[4]);
    } else if (argc == 3) {
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = 0x12345678u}, 6, 0, 0, 1, 0);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .initial_read_fault = READ_CHANGED_BODY},
                    6, 0, 0, 1, 0);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .initial_read_fault = READ_CHANGED_PATCH_WORD},
                    6, 0, 0, 1, 0);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .final_read_fault = READ_CHANGED_BODY},
                    16, 1, 1, 3, 0);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .final_read_fault = READ_CHANGED_PATCH_WORD},
                    16, 1, 1, 3, 0);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .final_raw_status = 7},
                    16, 1, 1, 3, 0);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .change_before_write = 1},
                    16, 1, 1, 2, 0);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .invalid_post_map = 1,
                          .post_app_physical = 14},
                    18, 1, 1, 3, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .post_raw_status = 7,
                          .post_app_physical = 14, .expected_post_physical = 14},
                    19, 1, 1, 4, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .post_raw_crc = RC0_CRC,
                          .post_app_physical = 14, .expected_post_physical = 14},
                    20, 1, 1, 4, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .post_raw_crc = RC0_CRC,
                          .post_app_physical = 15, .expected_post_physical = 15},
                    21, 1, 2, 4, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .post_raw_crc = RECORDER_CRC,
                          .post_app_physical = 14, .expected_post_physical = 14},
                    22, 1, 1, 4, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .post_raw_crc = 0x12345678u,
                          .post_app_physical = 14, .expected_post_physical = 14},
                    23, 1, 1, 4, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .writer_status = 7,
                          .invalid_post_map = 1, .post_app_physical = 14},
                    24, 1, 1, 3, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .writer_status = 7,
                          .post_raw_status = 7, .post_app_physical = 14,
                          .expected_post_physical = 14},
                    25, 1, 1, 4, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .writer_status = 7,
                          .post_raw_crc = RC0_CRC, .post_app_physical = 14,
                          .expected_post_physical = 14},
                    26, 1, 1, 4, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .writer_status = 7,
                          .post_raw_crc = RC0_CRC, .post_app_physical = 15,
                          .expected_post_physical = 15},
                    27, 1, 2, 4, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .writer_status = 7,
                          .post_raw_crc = RECORDER_CRC, .post_app_physical = 14,
                          .expected_post_physical = 14},
                    28, 1, 1, 4, 1);
        repair_case(stub, (size_t)length,
                    (Run){.source_crc = RC0_CRC, .writer_status = 7,
                          .post_raw_crc = 0x12345678u, .post_app_physical = 14,
                          .expected_post_physical = 14},
                    29, 1, 1, 4, 1);
        equal_non14_refusal_case(stub, (size_t)length);
        puts("PASS atomic physical-14 writer and exact physical readback packets");
    } else {
        uint32_t one_bad[] = {14};
        uint32_t two_bad[] = {0, 1};
        physical_pair_case(stub, (size_t)length, NULL, 0, NULL, 0,
                           14, 14, RC0_CRC, 12, 1, 1);
        physical_pair_case(stub, (size_t)length, NULL, 0, one_bad, 1,
                           14, 15, RC0_CRC, 9, 1, 2);
        physical_pair_case(stub, (size_t)length, NULL, 0, two_bad, 2,
                           14, 16, RC0_CRC, 10, 1, 3);
        physical_pair_case(stub, (size_t)length, one_bad, 1, NULL, 0,
                           15, 14, RC0_CRC, 11, 2, 2);
        physical_pair_case(stub, (size_t)length, NULL, 0, one_bad, 1,
                           14, 15, RECORDER_CRC, 13, 1, 2);
        physical_pair_case(stub, (size_t)length, NULL, 0, one_bad, 1,
                           14, 15, 0x12345678u, 14, 1, 2);
        invalid_boot_case(stub, (size_t)length);
        raw_rc0_case(stub, (size_t)length);
        refusal_cases(stub, (size_t)length);
        puts("PASS boot snapshot and exact boot/application physical-pair packets");
    }
    free(stub);
    return EXIT_SUCCESS;
}
