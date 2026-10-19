#ifndef FUJI_AF_RAM_PUSH_ABI_H
#define FUJI_AF_RAM_PUSH_ABI_H

#include <stdint.h>

#define RAM_PUSH_PUBLICATION_COUNT 250u
#define RAM_PUSH_SETUP_COUNT 167u
#define RAM_PUSH_ACTIVATION_COUNT 83u
#define RAM_PUSH_MAILBOX_MAGIC 0x31504d52u
#define RAM_PUSH_ABI_VERSION 2u
#define RAM_PUSH_EXPECTED_SERVICE_CORE 0x4003u

enum ram_push_operation {
    RAM_PUSH_INSTALL = 1,
    RAM_PUSH_ROLLBACK = 2,
    RAM_PUSH_STATUS = 3,
};

enum ram_push_status {
    RAM_PUSH_OK = 0,
    RAM_PUSH_BAD_CORE = -1,
    RAM_PUSH_BAD_ABI = -2,
    RAM_PUSH_OLD_MISMATCH = -4,
    RAM_PUSH_ROLLBACK_MISMATCH = -5,
    RAM_PUSH_PEER_FAILED = -6,
    RAM_PUSH_PARK_TIMEOUT = -7,
    RAM_PUSH_COMPLETION_TIMEOUT = -8,
};

enum ram_push_phase {
    RAM_PUSH_PHASE_SETUP = 0,
    RAM_PUSH_PHASE_ACTIVATION = 1,
};

struct ram_push_publication {
    uint32_t address;
    uint32_t old_word;
    uint32_t new_word;
    uint32_t phase;
};

struct ram_push_peer_report {
    uint32_t nonce;
    uint32_t core;
    int32_t status;
    uint32_t lines;
};


struct ram_push_mailbox {
    uint32_t magic;
    uint32_t abi;
    uint32_t nonce;
    uint32_t operation;
    int32_t status;
    uint32_t validated;
    uint32_t setup_stored;
    uint32_t activation_stored;
    uint32_t restored;
    uint32_t service_core;
    struct ram_push_peer_report peer;
    uint32_t spin_bound;
    uint32_t cpu0_ack_spins;
    uint32_t rendezvous_parked;
    uint32_t rendezvous_go;
    uint32_t rendezvous_completed;
    uint32_t cpu0_park_spins;
    uint32_t cpu1_spins;
    uint32_t cpu0_cycles;
    uint32_t cpu0_instructions;
    uint32_t cpu1_cycles;
    uint32_t cpu1_instructions;
    uint32_t reserved;
};

extern struct ram_push_publication ram_push_publications[RAM_PUSH_PUBLICATION_COUNT];
extern volatile struct ram_push_mailbox ram_push_mailbox;

int32_t ram_push_install(uint32_t nonce, uintptr_t *output, uint32_t *length);
int32_t ram_push_rollback(uint32_t nonce, uintptr_t *output, uint32_t *length);
int32_t ram_push_status(uint32_t nonce, uintptr_t *output, uint32_t *length);

#endif
