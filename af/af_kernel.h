#ifndef FUJI_AF_KERNEL_H
#define FUJI_AF_KERNEL_H
#include <stdbool.h>
#include <stdint.h>

/* Arbitrary synthetic integer lens units and synthetic-ms clock only.
 * These C structures are an in-process interface, never a wire/file format. */
typedef enum { AF_HOLD, AF_MOVE_TO, AF_MOVE_BY, AF_FINISH, AF_FAIL, AF_HAND_BACK } af_kind;
typedef enum {
    AF_START, AF_COARSE, AF_FINE, AF_CONFIRM, AF_CONFIRMED,
    AF_WAIT_COMPLETION, AF_WAIT_POSITION, AF_EARLY, AF_STALE, AF_COMMAND_MISMATCH,
    AF_INVALID, AF_DUPLICATE, AF_EXPOSURE, AF_CANCELLED, AF_MODE, AF_CLOCK,
    AF_STEP_LIMIT, AF_MEASUREMENT_LIMIT, AF_MOVE_LIMIT, AF_TIME_LIMIT,
    AF_LOW_CONFIDENCE, AF_CONFIRM_FAILED, AF_TERMINAL, AF_BOUNDS
} af_reason;
typedef struct {
    int32_t position_min, position_max;
    uint32_t coarse_step, fine_step, min_peak_score, min_contrast_span, confirmation_drop;
    uint32_t settle_ms, max_age_ms, max_moves, max_measurements, max_steps, max_duration_ms;
    bool require_measured_position, require_completion;
    uint32_t exposure_epoch;
} af_config;
typedef struct {
    uint32_t sequence_id, command_id_at_acquisition;
    int32_t commanded_position_at_acquisition;
    uint64_t acquired_at, delivered_at;
    bool position_known;
    int32_t reported_position;
    bool completion_known, reported_completion, valid, score_known;
    uint32_t sharpness, exposure_epoch;
    bool cancelled, supported_mode;
} af_observation;
typedef struct {
    af_kind kind;
    af_reason reason;
    uint32_t command_id, triggering_sequence_id, measurements, moves, steps;
    bool trigger_known, target_known, terminal;
    int32_t target_position;
} af_action;
typedef struct {
    af_config config;
    uint64_t started_at, last_delivery, command_issued_at;
    uint32_t command_id, last_sequence, measurements, moves, steps;
    uint32_t best_score, score_min, score_max;
    int32_t pending_position, best_position, fine_end;
    uint8_t phase; /* 0 coarse, 1 fine, 2 final verification, 3 terminal */
    bool sequence_seen, best_known;
} af_state;

/* Invalid configuration returns false without issuing an action. */
bool af_init(af_state *state, const af_config *config, uint64_t started_at, af_action *action);
void af_step(af_state *state, const af_observation *observation, af_action *action);
const char *af_kind_name(af_kind kind);
const char *af_reason_name(af_reason reason);
#endif
