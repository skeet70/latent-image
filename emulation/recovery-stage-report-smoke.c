#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/arm.h>
#include <unicorn/unicorn.h>

#define SD_BASE 0xfff50000u
#define STATE_BASE 0xfff00434u
#define MODE_WORD 0xfff98000u
#define REPORT_ENTRY 0x00006200u
#define REPORT_LBA 0x00020800u
#define EXIT_PC 0x00008000u

enum Scenario { SUCCESS, NO_READY, NO_WRITE_READY, TORN_WRITE, BAD_READBACK, R1_ERROR };
enum Transfer { IDLE, WRITING, READING };

typedef struct {
    enum Scenario scenario;
    enum Transfer transfer;
    uint16_t normal_status;
    uint16_t error_status;
    uint32_t argument;
    uint8_t sector[512];
    unsigned fifo_words;
    unsigned commands[4];
    unsigned command_count;
    int invalid_access;
    int interrupted;
    int reached_exit;
} Model;

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

static void store16(uc_engine *uc, uint64_t address, uint16_t value)
{
    require_uc(uc_mem_write(uc, address, &value, sizeof value));
}

static void store32(uc_engine *uc, uint64_t address, uint32_t value)
{
    require_uc(uc_mem_write(uc, address, &value, sizeof value));
}

static void mmio(uc_engine *uc, uc_mem_type type, uint64_t address,
                 int size, int64_t value, void *opaque)
{
    Model *model = opaque;
    uint32_t offset = (uint32_t)(address - SD_BASE);
    if (type == UC_MEM_READ) {
        if (offset == 0x10 && size == 4) {
            uint32_t response = 0x00000900u;
            if (model->scenario == R1_ERROR && model->command_count > 0 &&
                model->commands[model->command_count - 1] == 0x193au) response |= 0x80000000u;
            store32(uc, address, response);
        } else if (offset == 0x20 && size == 4 && model->transfer == READING && model->fifo_words < 128) {
            uint32_t word;
            memcpy(&word, model->sector + 4*model->fifo_words, sizeof word);
            if (model->scenario == BAD_READBACK && model->fifo_words == 0) word ^= 1u;
            store32(uc, address, word);
            model->fifo_words++;
            if (model->fifo_words == 128) model->normal_status |= 2u;
        } else if (offset == 0x24 && size == 4) {
            store32(uc, address, 0x00030000u);
        } else if (offset == 0x28 && size == 1) {
            uint8_t host = 2;
            require_uc(uc_mem_write(uc, address, &host, sizeof host));
        } else if (offset == 0x2c && size == 2) {
            store16(uc, address, 7);
        } else if (offset == 0x30 && size == 2) {
            store16(uc, address, model->normal_status);
        } else if (offset == 0x32 && size == 2) {
            store16(uc, address, model->error_status);
        } else {
            model->invalid_access = 1;
            require_uc(uc_emu_stop(uc));
        }
        return;
    }
    if (type != UC_MEM_WRITE) return;
    if (offset == 0x04 && size == 2) {
        if ((uint16_t)value != 0x0200u) model->invalid_access = 1;
    } else if (offset == 0x06 && size == 2) {
        if ((uint16_t)value != 1u) model->invalid_access = 1;
    } else if (offset == 0x08 && size == 4) {
        model->argument = (uint32_t)value;
    } else if (offset == 0x0c && size == 2) {
        if ((uint16_t)value != 0u && (uint16_t)value != 0x10u && (uint16_t)value != 0x26u) model->invalid_access = 1;
    } else if (offset == 0x0e && size == 2) {
        uint16_t command = (uint16_t)value;
        if (model->command_count < 4) model->commands[model->command_count++] = command;
        model->normal_status |= 1u;
        model->fifo_words = 0;
        if (command == 0x193au) {
            model->transfer = WRITING;
            if (model->scenario != NO_WRITE_READY) model->normal_status |= 0x10u;
        } else if (command == 0x113au) {
            model->transfer = READING;
            model->normal_status |= 0x20u;
        } else if (command == 0x0d1au) {
            model->transfer = IDLE;
        } else {
            model->invalid_access = 1;
        }
    } else if (offset == 0x20 && size == 4 && model->transfer == WRITING && model->fifo_words < 128) {
        uint32_t word = (uint32_t)value;
        memcpy(model->sector + 4*model->fifo_words, &word, sizeof word);
        model->fifo_words++;
        if (model->scenario == TORN_WRITE && model->fifo_words == 64) {
            model->interrupted = 1;
            require_uc(uc_emu_stop(uc));
        } else if (model->fifo_words == 128) {
            model->normal_status |= 2u;
        }
    } else if (offset == 0x30 && size == 2) {
        uint16_t acknowledged = (uint16_t)value;
        if (acknowledged != 0x33u && acknowledged != 1u && acknowledged != 2u &&
            acknowledged != 0x10u && acknowledged != 0x20u) model->invalid_access = 1;
        model->normal_status &= (uint16_t)~acknowledged;
    } else if (offset == 0x32 && size == 2) {
        model->error_status &= (uint16_t)~value;
    } else {
        model->invalid_access = 1;
        require_uc(uc_emu_stop(uc));
    }
}

static void code(uc_engine *uc, uint64_t address, uint32_t size, void *opaque)
{
    (void)size;
    Model *model = opaque;
    if (address == EXIT_PC) {
        model->reached_exit = 1;
        require_uc(uc_emu_stop(uc));
    }
}

static uint32_t load32(const uint8_t *bytes, size_t offset)
{
    uint32_t value;
    memcpy(&value, bytes + offset, sizeof value);
    return value;
}

static int valid_record(const uint8_t *sector, uint32_t stage)
{
    if (memcmp(sector, "XM1STAGE", 8) != 0 || load32(sector, 8) != 1u ||
        load32(sector, 12) != stage || load32(sector, 16) != ~stage ||
        load32(sector, 504) != 0x504d4f43u) return 0;
    uint32_t sum = 0;
    for (unsigned offset = 0; offset < 512; offset += 4) sum += load32(sector, offset);
    return sum == 0;
}

static void exercise(const uint8_t *image, size_t length, enum Scenario scenario,
                     uint32_t expected_return, int expect_record)
{
    uc_engine *uc;
    uc_hook mmio_hook, code_hook;
    Model model = {.scenario = scenario};
    require_uc(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    require_uc(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    require_uc(uc_mem_map(uc, 0, 0x10000, UC_PROT_ALL));
    require_uc(uc_mem_write(uc, 0x6000, image, length));
    require_uc(uc_mem_map(uc, 0xfff00000, 0x1000, UC_PROT_ALL));
    require_uc(uc_mem_map(uc, 0xfff50000, 0x1000, UC_PROT_ALL));
    require_uc(uc_mem_map(uc, 0xfff98000, 0x1000, UC_PROT_ALL));
    store32(uc, STATE_BASE + 0x00, 0x12340000u);
    store32(uc, STATE_BASE + 0x0c, 1u);
    store32(uc, STATE_BASE + 0x10, scenario == NO_READY ? 0u : 1u);
    store32(uc, MODE_WORD, 1u);
    uint32_t value = 2;
    require_uc(uc_reg_write(uc, UC_ARM_REG_R0, &value));
    value = 0xeb34a9c9u;
    require_uc(uc_reg_write(uc, UC_ARM_REG_R1, &value));
    value = 0xf000;
    require_uc(uc_reg_write(uc, UC_ARM_REG_SP, &value));
    value = EXIT_PC;
    require_uc(uc_reg_write(uc, UC_ARM_REG_LR, &value));
    for (int reg = UC_ARM_REG_R4; reg <= UC_ARM_REG_R11; reg++) {
        value = 0x11110000u + (uint32_t)reg;
        require_uc(uc_reg_write(uc, reg, &value));
    }
    value = 0x13;
    require_uc(uc_reg_write(uc, UC_ARM_REG_CPSR, &value));
    require_uc(uc_hook_add(uc, &mmio_hook, UC_HOOK_MEM_READ | UC_HOOK_MEM_WRITE,
                           (void *)mmio, &model, SD_BASE, SD_BASE + 0xfff));
    require_uc(uc_hook_add(uc, &code_hook, UC_HOOK_CODE, (void *)code, &model, 1, 0));
    require_uc(uc_emu_start(uc, REPORT_ENTRY, EXIT_PC + 4, 2000000, 2000000));
    require_uc(uc_reg_read(uc, UC_ARM_REG_R0, &value));
    if (model.invalid_access || (!model.interrupted && (!model.reached_exit || value != expected_return))) {
        fprintf(stderr, "scenario %d: invalid=%d interrupted=%d exit=%d r0=%u\n",
                scenario, model.invalid_access, model.interrupted, model.reached_exit, value);
        exit(EXIT_FAILURE);
    }
    for (int reg = UC_ARM_REG_R4; reg <= UC_ARM_REG_R11; reg++) {
        uint32_t observed;
        require_uc(uc_reg_read(uc, reg, &observed));
        if (!model.interrupted && observed != 0x11110000u + (uint32_t)reg) {
            fprintf(stderr, "scenario %d: callee-saved register changed\n", scenario);
            exit(EXIT_FAILURE);
        }
    }
    int record = valid_record(model.sector, 2);
    if (record != expect_record || (!model.interrupted && scenario != NO_READY && scenario != NO_WRITE_READY && model.argument != REPORT_LBA + 2)) {
        fprintf(stderr, "scenario %d: record=%d argument=%08x commands=%u\n",
                scenario, record, model.argument, model.command_count);
        exit(EXIT_FAILURE);
    }
    if (scenario == SUCCESS && (model.command_count != 4 || model.commands[0] != 0x0d1a ||
        model.commands[1] != 0x193a || model.commands[2] != 0x0d1a || model.commands[3] != 0x113a)) {
        fprintf(stderr, "success command sequence mismatch\n");
        exit(EXIT_FAILURE);
    }
    if (model.interrupted)
        printf("PASS scenario=%d outcome=interrupted record=torn words=%u\n", scenario, model.fifo_words);
    else
        printf("PASS scenario=%d return=%u record=%s words=%u\n",
               scenario, value, record ? "valid" : "absent", model.fifo_words);
    require_uc(uc_close(uc));
}

int main(int argc, char **argv)
{
    if (argc != 2) {
        fprintf(stderr, "Usage: recovery-stage-report-smoke linked-reporter.bin\n");
        return EXIT_FAILURE;
    }
    uint8_t image[0x1400];
    size_t length = read_image(argv[1], image, sizeof image);
    if (length == 0) {
        fprintf(stderr, "linked reporter image is empty\n");
        return EXIT_FAILURE;
    }
    exercise(image, length, SUCCESS, 0, 1);
    exercise(image, length, NO_READY, 1, 0);
    exercise(image, length, NO_WRITE_READY, 4, 0);
    exercise(image, length, TORN_WRITE, 0, 0);
    exercise(image, length, BAD_READBACK, 10, 1);
    exercise(image, length, R1_ERROR, 3, 0);
    return EXIT_SUCCESS;
}
