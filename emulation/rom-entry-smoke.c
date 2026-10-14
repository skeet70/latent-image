// Captured ROM dispatch, loader frames and handoff execute; storage bodies are substituted.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unicorn/unicorn.h>
#include <unicorn/arm.h>

typedef struct { uint32_t value; } ArmAddress;
typedef struct { const char *value; } ArtifactPath;
typedef struct { unsigned char bytes[0x4000]; } RomImage;
typedef struct { unsigned char bytes[0x1400]; size_t length; } GuardImage;
enum BootMode { NAND_BOOT, FAT_SD, RAW_SD, MEDIA_3, MEDIA_4, MEDIA_5, MEDIA_6, RECEIVER };
enum Outcome { UNFINISHED, ENTRY, REFUSED };
typedef struct {
    enum BootMode mode;
    ArmAddress body, epilogue, witness;
} Loader;
enum RomDomain { RESET_ALIAS, CAPTURED_ROM };
typedef struct {
    Loader loader;
    enum Outcome outcome;
    const GuardImage *guard;
    enum RomDomain domain;
    int wrong_stack;
    int corrupt_witness;
    int reached_vector;
} Run;

static void require_uc(uc_err error) {
    if (error != UC_ERR_OK) {
        fprintf(stderr, "Unicorn: %s\n", uc_strerror(error));
        exit(EXIT_FAILURE);
    }
}

static size_t read_image(ArtifactPath path, unsigned char *bytes, size_t capacity) {
    FILE *file = fopen(path.value, "rb");
    if (!file) { perror(path.value); exit(EXIT_FAILURE); }
    size_t length = fread(bytes, 1, capacity, file);
    int extra = fgetc(file);
    if (ferror(file) || extra != EOF || fclose(file) != 0) {
        fprintf(stderr, "Invalid or oversized input: %s\n", path.value);
        exit(EXIT_FAILURE);
    }
    return length;
}

static void trace(uc_engine *uc, uint64_t pc, uint32_t size, void *opaque) {
    (void)size;
    Run *run = opaque;
    uint32_t value;
    uint64_t canonical = pc | 0x80000000;
    if (canonical == 0x80000260) {
        value = (uint32_t)run->loader.mode;
        require_uc(uc_reg_write(uc, UC_ARM_REG_R0, &value));
        require_uc(uc_reg_read(uc, UC_ARM_REG_LR, &value));
        require_uc(uc_reg_write(uc, UC_ARM_REG_PC, &value));
    } else if (canonical == run->loader.body.value) {
        value = 0;
        require_uc(uc_reg_write(uc, UC_ARM_REG_R0, &value));
        value = 0x80002200;
        require_uc(uc_reg_write(uc, UC_ARM_REG_LR, &value));
        value = run->loader.epilogue.value;
        if (run->domain == RESET_ALIAS) value &= 0x7fffffff;
        require_uc(uc_reg_write(uc, UC_ARM_REG_PC, &value));
    } else if (pc == 0x80002298) {
        uint32_t vector = 0xe59ff018, destination = 0x6000;
        require_uc(uc_mem_write(uc, 0, &vector, sizeof vector));
        require_uc(uc_mem_write(uc, 0x20, &destination, sizeof destination));
    } else if (pc == 0) {
        uint32_t sp, witness;
        require_uc(uc_reg_read(uc, UC_ARM_REG_SP, &sp));
        require_uc(uc_mem_read(uc, 0xfff00efc, &witness, sizeof witness));
        uint32_t expected = run->loader.witness.value;
        if (run->domain == RESET_ALIAS) expected &= 0x7fffffff;
        if (sp != 0xfff00f00 || witness != expected) {
            fprintf(stderr, "Mode %u: unexpected SP/witness %08x/%08x\n",
                    (unsigned)run->loader.mode, sp, witness);
            exit(EXIT_FAILURE);
        }
        run->reached_vector = 1;
        value = 0xf80000d3;
        require_uc(uc_reg_write(uc, UC_ARM_REG_CPSR, &value));
        value = 0xdeadbeef;
        require_uc(uc_reg_write(uc, UC_ARM_REG_R0, &value));
        if (!run->guard) {
            run->outcome = ENTRY;
            require_uc(uc_emu_stop(uc));
        } else if (run->wrong_stack) {
            value = sp - 8;
            require_uc(uc_reg_write(uc, UC_ARM_REG_SP, &value));
        } else if (run->corrupt_witness) {
            value = 0;
            require_uc(uc_mem_write(uc, 0xfff00efc, &value, sizeof value));
        }
    } else if (pc == 0x6200) {
        value = 0;
        require_uc(uc_reg_write(uc, UC_ARM_REG_R0, &value));
        require_uc(uc_reg_read(uc, UC_ARM_REG_LR, &value));
        require_uc(uc_reg_write(uc, UC_ARM_REG_PC, &value));
    } else if (pc == 0x1800) {
        uint32_t flags;
        require_uc(uc_reg_read(uc, UC_ARM_REG_R0, &value));
        require_uc(uc_reg_read(uc, UC_ARM_REG_CPSR, &flags));
        if (value != 0 || (flags & 0xf8000000) != 0) {
            fprintf(stderr, "Guard changed native-entry R0/APSR: %08x/%08x\n", value, flags);
            exit(EXIT_FAILURE);
        }
        run->outcome = ENTRY;
        require_uc(uc_emu_stop(uc));
    } else if (pc >= 0x6000 && pc < 0x6040) {
        require_uc(uc_mem_read(uc, pc, &value, sizeof value));
        if (value == 0xe320f003) {
            run->outcome = REFUSED;
            require_uc(uc_emu_stop(uc));
        }
    }
}

static void exercise(const RomImage *rom, const GuardImage *guard, Loader loader,
                     enum RomDomain domain, enum BootMode admitted_mode,
                     int wrong_stack, int corrupt_witness) {
    uc_engine *uc;
    uc_hook hook;
    Run run = {.loader = loader, .outcome = UNFINISHED, .guard = guard,
               .domain = domain, .wrong_stack = wrong_stack,
               .corrupt_witness = corrupt_witness};
    require_uc(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    require_uc(uc_ctl_set_cpu_model(uc, UC_CPU_ARM_CORTEX_R5F));
    require_uc(uc_mem_map(uc, 0x80000000, 0x4000, UC_PROT_ALL));
    require_uc(uc_mem_write(uc, 0x80000000, rom->bytes, sizeof rom->bytes));
    require_uc(uc_mem_map(uc, 0xfff00000, 0x1000, UC_PROT_ALL));
    require_uc(uc_mem_map(uc, 0xfff88000, 0x12000, UC_PROT_ALL));
    require_uc(uc_mem_map(uc, 0, 0x8000, UC_PROT_ALL));
    require_uc(uc_mem_write(uc, 0, rom->bytes, sizeof rom->bytes));
    if (guard) require_uc(uc_mem_write(uc, 0x6000, guard->bytes, guard->length));
    uint32_t value = 0xd3;
    require_uc(uc_reg_write(uc, UC_ARM_REG_CPSR, &value));
    require_uc(uc_emu_start(uc, 0x28, 0xe4, 1000000, 128));
    require_uc(uc_reg_read(uc, UC_ARM_REG_SP, &value));
    if (value != 0xfff00f00) {
        fprintf(stderr, "Unexpected ROM-initialized SP %08x\n", value);
        exit(EXIT_FAILURE);
    }
    // Late samples conflict with FAT selection and would admit other paths in the old guard.
    value = loader.mode == FAT_SD ? 0xc00 : 0;
    require_uc(uc_mem_write(uc, 0xfff89014, &value, sizeof value));
    value = 1;
    require_uc(uc_mem_write(uc, 0xfff98000, &value, sizeof value));
    value = domain == RESET_ALIAS ? 0x130 : 0x80000130;
    require_uc(uc_reg_write(uc, UC_ARM_REG_PC, &value));
    require_uc(uc_hook_add(uc, &hook, UC_HOOK_CODE, (void *)trace, &run, 1, 0));
    uc_err execution = uc_emu_start(uc, value, 0xffffffff, 1000000, 200);
    if (execution != UC_ERR_OK) {
        require_uc(uc_reg_read(uc, UC_ARM_REG_PC, &value));
        uint32_t cpsr;
        uc_arm_cp_reg control = {.cp = 15, .crn = 1};
        require_uc(uc_reg_read(uc, UC_ARM_REG_CPSR, &cpsr));
        require_uc(uc_reg_read(uc, UC_ARM_REG_CP_REG, &control));
        fprintf(stderr, "CPSR=%08x SCTLR=%08llx\n", cpsr, (unsigned long long)control.val);
        fprintf(stderr, "Mode %u stopped at %08x\n", (unsigned)loader.mode, value);
        require_uc(execution);
    }
    enum Outcome expected = !guard || (loader.mode == admitted_mode && !wrong_stack && !corrupt_witness)
        ? ENTRY : REFUSED;
    if (!run.reached_vector || run.outcome != expected) {
        fprintf(stderr, "Mode %u stack=%d tag=%d: outcome %d, expected %d\n",
                (unsigned)loader.mode, wrong_stack, corrupt_witness, run.outcome, expected);
        exit(EXIT_FAILURE);
    }
    uint32_t witness = loader.witness.value;
    if (domain == RESET_ALIAS) witness &= 0x7fffffff;
    printf("PASS domain=%s mode=%u saved-return=%08x stack=%s tag=%s outcome=%s\n",
           domain == RESET_ALIAS ? "reset-alias" : "captured-ROM",
           (unsigned)loader.mode, witness, wrong_stack ? "changed" : "ROM",
           corrupt_witness ? "changed" : "ROM", run.outcome == ENTRY ? "entry" : "refused");
    require_uc(uc_close(uc));
}

int main(int argc, char **argv) {
    if (argc != 2 && argc != 3 && !(argc == 4 && strcmp(argv[3], "--raw") == 0)) {
        fprintf(stderr, "Usage: rom-entry-smoke ROM-4000.bin [GUARD.bin [--raw]]\n");
        return EXIT_FAILURE;
    }
    RomImage rom;
    if (read_image((ArtifactPath){argv[1]}, rom.bytes, sizeof rom.bytes) != sizeof rom.bytes) {
        fprintf(stderr, "ROM must contain exactly 0x4000 bytes\n");
        return EXIT_FAILURE;
    }
    GuardImage guard;
    if (argc >= 3) {
        guard.length = read_image((ArtifactPath){argv[2]}, guard.bytes, sizeof guard.bytes);
        if (guard.length < 0x40 || guard.length % 4 != 0) return EXIT_FAILURE;
    }
    const Loader loaders[] = {
        {NAND_BOOT, {0x80000b30}, {0x80000b34}, {0x80000144}},
        {FAT_SD, {0x80000830}, {0x80000934}, {0x8000015c}},
        {RAW_SD, {0x80000720}, {0x80000828}, {0x80000174}},
        {MEDIA_3, {0x8000093c}, {0x80000a64}, {0x8000018c}},
        {MEDIA_4, {0x8000093c}, {0x80000a64}, {0x800001a4}},
        {MEDIA_5, {0x8000093c}, {0x80000a64}, {0x800001bc}},
        {MEDIA_6, {0x8000093c}, {0x80000a64}, {0x800001d4}},
        {RECEIVER, {0x80000ac4}, {0x80000b14}, {0x800001ec}},
    };
    enum BootMode admitted_mode = argc == 4 ? RAW_SD : FAT_SD;
    for (enum RomDomain domain = RESET_ALIAS; domain <= CAPTURED_ROM; ++domain) {
        for (size_t i = 0; i < sizeof loaders / sizeof loaders[0]; ++i)
            exercise(&rom, argc >= 3 ? &guard : NULL, loaders[i], domain, admitted_mode, 0, 0);
        if (argc >= 3) {
            exercise(&rom, &guard, loaders[admitted_mode], domain, admitted_mode, 1, 0);
            exercise(&rom, &guard, loaders[admitted_mode], domain, admitted_mode, 0, 1);
        }
    }
    return EXIT_SUCCESS;
}
