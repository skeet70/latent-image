#include "ram_push_abi.h"

__attribute__((section(".ram_push.publications"), aligned(4), used))
struct ram_push_publication ram_push_publications[RAM_PUSH_PUBLICATION_COUNT] = {{0}};

__attribute__((section(".ram_push.mailbox"), aligned(32), used))
volatile struct ram_push_mailbox ram_push_mailbox;

extern uint8_t __ram_push_start[];
extern uint8_t __ram_push_end[];

typedef int32_t (*core1_action_fn)(void *payload, uint32_t length);
typedef int32_t (*core1_submit_fn)(uint32_t phase, core1_action_fn action, const void *source, uint32_t length);
typedef int32_t (*core1_range_fn)(uint32_t phase, uintptr_t address, uint32_t length);
typedef int32_t (*worker_post_fn)(uint32_t channel, const void *request);
typedef void (*worker_completion_fn)(void *cookie, uint32_t origin, int32_t result);
typedef void (*local_range_fn)(uintptr_t address, uint32_t length);

struct peer_action_payload {
    uint32_t nonce;
    uint32_t service_core;
    const struct ram_push_publication *publications;
    uint32_t begin;
    uint32_t end;
    uintptr_t runtime_begin;
    uint32_t runtime_length;
    volatile struct ram_push_peer_report *report;
};

/* Each rendezvous line has one writing core while an operation runs. */
struct command_line {
    volatile uint32_t value;
    uint32_t nonce;
    uint32_t operation;
    uint32_t service_core;
    uint32_t padding[4];
};

struct signal_line {
    volatile uint32_t value;
    uint32_t padding[7];
};

struct report_line {
    volatile struct ram_push_peer_report report;
    uint32_t padding[4];
};

struct rendezvous_payload {
    struct command_line command;
    struct signal_line response;
    struct signal_line completed;
    struct report_line peer;
};

struct worker_inner {
    uint32_t origin;
    uint32_t phase;
    core1_action_fn action;
    void *argument;
    uint32_t length;
    worker_completion_fn completion;
    void *cookie;
};

struct __attribute__((packed)) worker_outer {
    uint16_t command;
    uint8_t source;
    uint8_t destination;
    uint32_t length;
    const struct worker_inner *inner;
};

__attribute__((section(".ram_push.rendezvous"), aligned(32), used))
static struct rendezvous_payload rendezvous;

#define CORE1_SUBMIT ((core1_submit_fn)(uintptr_t)0x006b0aa8u)
#define CORE1_RANGE ((core1_range_fn)(uintptr_t)0x006b0c3cu)
#define WORKER_POST ((worker_post_fn)(uintptr_t)0x0072503cu)
#define LOCAL_RANGE ((local_range_fn)(uintptr_t)0x00042634u)
#define CORE1_CACHE_CONTROL ((core1_action_fn)(uintptr_t)0x0061c490u)
#define WORKER_PHASE 6u
#define WORKER_CHANNEL 0x58u
#define USB_TASK 0x24u
#define WORKER_COMMAND 0x6800u
/*
 * One wait iteration is a line clean+invalidate, a DSB and a reload of that
 * line from memory. At 2 us per iteration a full bound is 1.05 s, so CPU0's
 * two IRQ-masked waits together stay under 2.1 s and every bounded wait ends
 * inside the host's 5000 ms transfer timeout.
 */
#define RENDEZVOUS_SPIN_BOUND 0x80000u

/* command line, written by CPU0 */
enum rendezvous_command {
    RENDEZVOUS_IDLE = 0,
    RENDEZVOUS_COMMIT = 2,
    RENDEZVOUS_GO = 4,
    RENDEZVOUS_CANCEL = 5,
};

/* response line, written by CPU1 */
enum rendezvous_response {
    RENDEZVOUS_PARKED = 1,
    RENDEZVOUS_ACKNOWLEDGED = 3,
    RENDEZVOUS_LEFT = 6,
};

static inline uint32_t core_identity(void) {
    uint32_t identity;
    __asm__ volatile("mrc p14, 0, %0, c2, c0, 0" : "=r"(identity));
    return identity;
}
static inline uint32_t cycle_counter(void) {
    uint32_t cycles;
    __asm__ volatile("mrc p15, 0, %0, c9, c13, 0" : "=r"(cycles));
    return cycles;
}


static inline uint32_t mask_interrupts(void) {
    uint32_t status;
    __asm__ volatile("mrs %0, cpsr\n\tcpsid i" : "=r"(status) : : "memory");
    return status;
}

static inline void restore_interrupts(uint32_t status) {
    if ((status & 0x80u) == 0u) {
        __asm__ volatile("cpsie i" : : : "memory");
    }
}

static inline void clean_invalidate_line(uintptr_t address) {
    __asm__ volatile("mcr p15, 0, %0, c7, c14, 1" : : "r"(address) : "memory");
}

static inline void synchronize_line(uintptr_t address) {
    clean_invalidate_line(address & ~31u);
    __asm__ volatile("dsb sy" : : : "memory");
}

static uint32_t load_signal(volatile uint32_t *signal) {
    synchronize_line((uintptr_t)signal);
    return *signal;
}

static void store_signal(volatile uint32_t *signal, uint32_t value) {
    *signal = value;
    synchronize_line((uintptr_t)signal);
}

static void synchronize_mailbox(void) {
    uintptr_t begin = (uintptr_t)&ram_push_mailbox & ~31u;
    uintptr_t end = ((uintptr_t)&ram_push_mailbox + sizeof(ram_push_mailbox) + 31u) & ~31u;
    for (uintptr_t line = begin; line < end; line += 32u) {
        clean_invalidate_line(line);
    }
    __asm__ volatile("dsb sy" : : : "memory");
}

static void publish_mailbox(uintptr_t *output, uint32_t *length) {
    synchronize_mailbox();
    *output = (uintptr_t)&ram_push_mailbox;
    *length = sizeof(ram_push_mailbox);
}

static int32_t begin_operation(uint32_t nonce, uint32_t operation, uint32_t core) {
    volatile uint32_t *words = (volatile uint32_t *)&ram_push_mailbox;
    for (uint32_t index = 0; index < sizeof(ram_push_mailbox) / 4u; ++index) {
        words[index] = 0;
    }
    ram_push_mailbox.magic = RAM_PUSH_MAILBOX_MAGIC;
    ram_push_mailbox.abi = RAM_PUSH_ABI_VERSION;
    ram_push_mailbox.nonce = nonce;
    ram_push_mailbox.operation = operation;
    ram_push_mailbox.service_core = core;
    ram_push_mailbox.peer.nonce = nonce;
    ram_push_mailbox.peer.status = RAM_PUSH_PEER_FAILED;
    ram_push_mailbox.spin_bound = RENDEZVOUS_SPIN_BOUND;
    if (core != RAM_PUSH_EXPECTED_SERVICE_CORE) {
        return RAM_PUSH_BAD_CORE;
    }
    return RAM_PUSH_OK;
}

static int line_seen_before(
    const struct ram_push_publication *publications,
    uint32_t begin,
    uint32_t index) {
    uintptr_t line = publications[index].address & ~31u;
    for (uint32_t prior = begin; prior < index; ++prior) {
        if ((publications[prior].address & ~31u) == line) {
            return 1;
        }
    }
    return 0;
}

static void synchronize_current_core(void) {
    for (uint32_t index = 0; index < RAM_PUSH_PUBLICATION_COUNT; ++index) {
        if (!line_seen_before(ram_push_publications, 0, index)) {
            clean_invalidate_line(ram_push_publications[index].address & ~31u);
        }
    }
    __asm__ volatile("dsb sy\n\tmov r0, #0\n\tmcr p15, 0, r0, c7, c5, 0\n\tdsb sy\n\tisb sy" : : : "r0", "memory");
}

static void write_peer_report(
    volatile struct ram_push_peer_report *report,
    uint32_t nonce,
    uint32_t core,
    int32_t status,
    uint32_t lines) {
    report->nonce = nonce;
    report->core = core;
    report->status = status;
    report->lines = lines;
    synchronize_line((uintptr_t)report);
}

/* Copies CPU1's report line into the CPU0-owned mailbox. */
static int32_t read_peer_report(uint32_t nonce) {
    synchronize_line((uintptr_t)&rendezvous.peer);
    uint32_t reported_nonce = rendezvous.peer.report.nonce;
    uint32_t core = rendezvous.peer.report.core;
    int32_t status = rendezvous.peer.report.status;
    uint32_t lines = rendezvous.peer.report.lines;
    ram_push_mailbox.peer.nonce = reported_nonce;
    ram_push_mailbox.peer.core = core;
    ram_push_mailbox.peer.status = status;
    ram_push_mailbox.peer.lines = lines;
    if (status != RAM_PUSH_OK || reported_nonce != nonce ||
        core == ram_push_mailbox.service_core || lines != RAM_PUSH_PUBLICATION_COUNT) {
        return RAM_PUSH_PEER_FAILED;
    }
    return RAM_PUSH_OK;
}

static int32_t peer_cache_action(void *raw, uint32_t length) {
    struct peer_action_payload *payload = raw;
    if (length != sizeof(*payload) || payload->report == 0 ||
        payload->publications != ram_push_publications ||
        payload->begin > payload->end || payload->end > RAM_PUSH_PUBLICATION_COUNT) {
        return RAM_PUSH_BAD_ABI;
    }
    if (payload->runtime_length != 0u) {
        LOCAL_RANGE(payload->runtime_begin, payload->runtime_length);
    }
    for (uint32_t index = payload->begin; index < payload->end; ++index) {
        if (!line_seen_before(payload->publications, payload->begin, index)) {
            LOCAL_RANGE(payload->publications[index].address & ~31u, 32u);
        }
    }
    __asm__ volatile("dsb sy\n\tmov r0, #0\n\tmcr p15, 0, r0, c7, c5, 0\n\tdsb sy\n\tisb sy" : : : "r0", "memory");
    uint32_t identity = core_identity();
    int32_t status = identity == payload->service_core ? RAM_PUSH_BAD_CORE : RAM_PUSH_OK;
    write_peer_report(payload->report, payload->nonce, identity, status, payload->end - payload->begin);
    return status;
}

static int32_t synchronize_peer(uint32_t nonce) {
    struct peer_action_payload payload = {
        nonce,
        ram_push_mailbox.service_core,
        ram_push_publications,
        0,
        RAM_PUSH_PUBLICATION_COUNT,
        (uintptr_t)__ram_push_start,
        (uint32_t)(__ram_push_end - __ram_push_start),
        &rendezvous.peer.report,
    };
    write_peer_report(&rendezvous.peer.report, nonce, 0, RAM_PUSH_PEER_FAILED, 0);
    int32_t status = CORE1_SUBMIT(WORKER_PHASE, peer_cache_action, &payload, sizeof(payload));
    int32_t report = read_peer_report(nonce);
    return status == RAM_PUSH_OK ? report : RAM_PUSH_PEER_FAILED;
}

static int32_t prepare_peer(uint32_t nonce) {
    if (CORE1_RANGE(
            WORKER_PHASE,
            (uintptr_t)__ram_push_start,
            (uint32_t)(__ram_push_end - __ram_push_start)) != RAM_PUSH_OK) {
        return RAM_PUSH_PEER_FAILED;
    }
    if (CORE1_SUBMIT(WORKER_PHASE, CORE1_CACHE_CONTROL, 0, 0) != 4) {
        return RAM_PUSH_PEER_FAILED;
    }
    return synchronize_peer(nonce);
}

__attribute__((noinline)) static int32_t validate_words(uint32_t operation) {
    for (uint32_t index = 0; index < RAM_PUSH_PUBLICATION_COUNT; ++index) {
        const struct ram_push_publication *publication = &ram_push_publications[index];
        uint32_t word = *(volatile const uint32_t *)(uintptr_t)publication->address;
        if (word != publication->old_word &&
            (operation == RAM_PUSH_INSTALL || word != publication->new_word)) {
            return operation == RAM_PUSH_INSTALL ? RAM_PUSH_OLD_MISMATCH : RAM_PUSH_ROLLBACK_MISMATCH;
        }
        ram_push_mailbox.validated++;
    }
    return RAM_PUSH_OK;
}

__attribute__((noinline)) static int32_t validate_old_words(void) {
    return validate_words(RAM_PUSH_INSTALL);
}

__attribute__((noinline)) static int32_t validate_rollback_words(void) {
    return validate_words(RAM_PUSH_ROLLBACK);
}

__attribute__((noinline)) static void store_new_words(void) {
    for (uint32_t index = 0; index < RAM_PUSH_PUBLICATION_COUNT; ++index) {
        const struct ram_push_publication *publication = &ram_push_publications[index];
        *(volatile uint32_t *)(uintptr_t)publication->address = publication->new_word;
        if (index < RAM_PUSH_SETUP_COUNT) {
            ram_push_mailbox.setup_stored++;
        } else {
            ram_push_mailbox.activation_stored++;
        }
    }
}

__attribute__((noinline)) static void restore_old_words(void) {
    for (uint32_t index = RAM_PUSH_PUBLICATION_COUNT; index > 0; --index) {
        const struct ram_push_publication *publication = &ram_push_publications[index - 1u];
        *(volatile uint32_t *)(uintptr_t)publication->address = publication->old_word;
        ram_push_mailbox.restored++;
    }
}

__attribute__((noinline, used))
int32_t ram_push_rendezvous_locked(uint32_t operation) {
    return operation == RAM_PUSH_INSTALL ? validate_old_words() : validate_rollback_words();
}

__attribute__((noinline)) static void publish_words_locked(uint32_t operation) {
    if (operation == RAM_PUSH_INSTALL) {
        store_new_words();
    } else {
        restore_old_words();
    }
    synchronize_current_core();
}


__attribute__((noinline, used))
void ram_push_cpu0_window_exit(void) {
    __asm__ volatile("" : : : "memory");
}

__attribute__((noinline, used))
void ram_push_cpu1_window_exit(void) {
    __asm__ volatile("" : : : "memory");
}

__attribute__((noinline, used))
static int32_t peer_park_action(void *raw, uint32_t length) {
    struct rendezvous_payload *payload = raw;
    if (payload != &rendezvous || length != 0u) {
        return RAM_PUSH_BAD_ABI;
    }
    uint32_t cycle_start = cycle_counter();
    uint32_t interrupt_status = mask_interrupts();
    synchronize_current_core();
    uint32_t identity = core_identity();
    uint32_t command = load_signal(&payload->command.value);
    uint32_t nonce = payload->command.nonce;
    int32_t status = identity == payload->command.service_core ? RAM_PUSH_BAD_CORE : RAM_PUSH_OK;
    write_peer_report(&payload->peer.report, nonce, identity, status, 0);

    uint32_t spins = 0;
    if (status == RAM_PUSH_OK && command == RENDEZVOUS_IDLE) {
        store_signal(&payload->response.value, RENDEZVOUS_PARKED);
        while (spins < RENDEZVOUS_SPIN_BOUND) {
            command = load_signal(&payload->command.value);
            if (command == RENDEZVOUS_COMMIT || command == RENDEZVOUS_CANCEL) {
                break;
            }
            ++spins;
        }
    }
    uint32_t lines = 0;
    if (command == RENDEZVOUS_COMMIT) {
        store_signal(&payload->response.value, RENDEZVOUS_ACKNOWLEDGED);
        for (;;) {
            command = load_signal(&payload->command.value);
            if (command == RENDEZVOUS_GO || command == RENDEZVOUS_CANCEL) {
                break;
            }
            ++spins;
        }
        if (command == RENDEZVOUS_GO) {
            synchronize_current_core();
            ram_push_cpu1_window_exit();
            lines = RAM_PUSH_PUBLICATION_COUNT;
        }
    } else {
        store_signal(&payload->response.value, RENDEZVOUS_LEFT);
    }
    if (status == RAM_PUSH_OK && command != RENDEZVOUS_GO) {
        status = RAM_PUSH_PARK_TIMEOUT;
    }
    write_peer_report(&payload->peer.report, nonce, identity, status, lines);
    payload->completed.padding[0] = spins;
    payload->completed.padding[1] = cycle_counter() - cycle_start;
    restore_interrupts(interrupt_status);
    return status;
}

__attribute__((noinline, used))
static void peer_park_complete(void *cookie, uint32_t origin, int32_t result) {
    struct rendezvous_payload *payload = cookie;
    if (payload == &rendezvous && origin == USB_TASK) {
        (void)result;
        store_signal(&payload->completed.value, payload->command.nonce);
    }
}

static const struct worker_inner peer_park_job = {
    USB_TASK, WORKER_PHASE, peer_park_action, &rendezvous, 0, peer_park_complete, &rendezvous,
};

static const struct worker_outer peer_park_request = {
    WORKER_COMMAND, USB_TASK, WORKER_CHANNEL, sizeof(peer_park_job), &peer_park_job,
};

__attribute__((noinline, used))
static int post_peer_park(void) {
    return WORKER_POST(WORKER_CHANNEL, &peer_park_request) == RAM_PUSH_OK;
}

__attribute__((noinline, used))
static int wait_for_signal(volatile uint32_t *signal, uint32_t first, uint32_t second, uint32_t *spins) {
    uint32_t count = 0;
    int value = -1;
    for (; count < RENDEZVOUS_SPIN_BOUND; ++count) {
        uint32_t current = load_signal(signal);
        if (current == first || current == second) {
            value = (int)current;
            break;
        }
    }
    *spins = count;
    return value;
}

__attribute__((noinline, used))
static int32_t run_rendezvous(uint32_t nonce, uint32_t operation) {
    rendezvous.command.value = RENDEZVOUS_IDLE;
    rendezvous.command.nonce = nonce;
    rendezvous.command.service_core = ram_push_mailbox.service_core;
    rendezvous.response.value = RENDEZVOUS_IDLE;
    rendezvous.completed.value = 0;
    synchronize_line((uintptr_t)&rendezvous.command);
    synchronize_line((uintptr_t)&rendezvous.response);
    synchronize_line((uintptr_t)&rendezvous.completed);
    write_peer_report(&rendezvous.peer.report, nonce, 0, RAM_PUSH_PEER_FAILED, 0);
    if (!post_peer_park()) {
        return RAM_PUSH_PEER_FAILED;
    }

    uint32_t spins = 0;
    uint32_t cycle_start = cycle_counter();
    uint32_t interrupt_status = mask_interrupts();
    int32_t status = wait_for_signal(
        &rendezvous.response.value,
        RENDEZVOUS_PARKED,
        RENDEZVOUS_LEFT,
        &spins) == RENDEZVOUS_PARKED ? RAM_PUSH_OK : RAM_PUSH_PARK_TIMEOUT;
    ram_push_mailbox.cpu0_park_spins = spins;
    if (status == RAM_PUSH_OK) {
        ram_push_mailbox.rendezvous_parked = nonce;
        status = ram_push_rendezvous_locked(operation);
    }
    if (status == RAM_PUSH_OK) {
        store_signal(&rendezvous.command.value, RENDEZVOUS_COMMIT);
        int acknowledged = wait_for_signal(
            &rendezvous.response.value,
            RENDEZVOUS_ACKNOWLEDGED,
            RENDEZVOUS_LEFT,
            &spins);
        ram_push_mailbox.cpu0_ack_spins = spins;
        if (acknowledged == RENDEZVOUS_ACKNOWLEDGED) {
            publish_words_locked(operation);
            ram_push_cpu0_window_exit();
        } else {
            status = RAM_PUSH_PARK_TIMEOUT;
        }
    }
    store_signal(
        &rendezvous.command.value,
        status == RAM_PUSH_OK ? RENDEZVOUS_GO : RENDEZVOUS_CANCEL);
    if (status == RAM_PUSH_OK) {
        ram_push_mailbox.rendezvous_go = nonce;
    }
    ram_push_mailbox.cpu0_cycles = cycle_counter() - cycle_start;
    restore_interrupts(interrupt_status);

    if (wait_for_signal(&rendezvous.completed.value, nonce, nonce, &spins) != (int)nonce) {
        return RAM_PUSH_COMPLETION_TIMEOUT;
    }
    ram_push_mailbox.rendezvous_completed = nonce;
    ram_push_mailbox.cpu1_spins = rendezvous.completed.padding[0];
    ram_push_mailbox.cpu1_cycles = rendezvous.completed.padding[1];
    int32_t report = read_peer_report(nonce);
    if (status == RAM_PUSH_OK && report != RAM_PUSH_OK) {
        return RAM_PUSH_PEER_FAILED;
    }
    return status;
}

static int32_t validate_layout(void) {
    for (uint32_t index = 0; index < RAM_PUSH_PUBLICATION_COUNT; ++index) {
        const struct ram_push_publication *publication = &ram_push_publications[index];
        uint32_t expected_phase = index < RAM_PUSH_SETUP_COUNT ? RAM_PUSH_PHASE_SETUP : RAM_PUSH_PHASE_ACTIVATION;
        if ((publication->address & 3u) != 0u || publication->phase != expected_phase) {
            return RAM_PUSH_BAD_ABI;
        }
    }
    return RAM_PUSH_OK;
}

static int32_t execute_operation(uint32_t nonce, uint32_t operation) {
    int32_t status = begin_operation(nonce, operation, core_identity());
    if (status == RAM_PUSH_OK) {
        status = validate_layout();
    }
    if (status == RAM_PUSH_OK) {
        status = prepare_peer(nonce);
    }
    if (status == RAM_PUSH_OK) {
        status = run_rendezvous(nonce, operation);
    }
    ram_push_mailbox.status = status;
    return status;
}

int32_t ram_push_install(uint32_t nonce, uintptr_t *output, uint32_t *length) {
    (void)execute_operation(nonce, RAM_PUSH_INSTALL);
    publish_mailbox(output, length);
    return 0;
}

int32_t ram_push_rollback(uint32_t nonce, uintptr_t *output, uint32_t *length) {
    (void)execute_operation(nonce, RAM_PUSH_ROLLBACK);
    publish_mailbox(output, length);
    return 0;
}

int32_t ram_push_status(uint32_t nonce, uintptr_t *output, uint32_t *length) {
    (void)nonce;
    publish_mailbox(output, length);
    return 0;
}
