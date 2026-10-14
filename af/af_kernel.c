#include "af_kernel.h"
#include <limits.h>
#include <string.h>

static void emit(const af_state *s, af_action *a, af_kind kind, af_reason reason,
                 bool trigger_known, uint32_t sequence) {
    *a = (af_action){.kind = kind, .reason = reason, .command_id = s->command_id,
        .triggering_sequence_id = sequence, .trigger_known = trigger_known,
        .target_known = kind == AF_MOVE_TO || kind == AF_FINISH,
        .target_position = kind == AF_FINISH ? s->best_position : s->pending_position,
        .measurements = s->measurements, .moves = s->moves, .steps = s->steps,
        .terminal = s->phase == 3};
}

static void stop(af_state *s, af_action *a, af_kind kind, af_reason reason, uint32_t sequence) {
    s->phase = 3;
    emit(s, a, kind, reason, true, sequence);
}

static bool issue(af_state *s, af_action *a, int64_t target, uint64_t now,
                  af_reason reason, bool trigger_known, uint32_t sequence) {
    if (target < s->config.position_min || target > s->config.position_max ||
        s->command_id == UINT32_MAX) {
        stop(s, a, AF_FAIL, AF_BOUNDS, sequence);
        return false;
    }
    if (s->moves >= s->config.max_moves) {
        stop(s, a, AF_FAIL, AF_MOVE_LIMIT, sequence);
        return false;
    }
    s->pending_position = (int32_t)target;
    s->command_id++;
    s->moves++;
    s->command_issued_at = now;
    emit(s, a, AF_MOVE_TO, reason, trigger_known, sequence);
    return true;
}

bool af_init(af_state *s, const af_config *c, uint64_t started_at, af_action *a) {
    if (!s || !c || !a) return false;
    const int64_t span = (int64_t)c->position_max - c->position_min;
    if (span < 1 || span > 1000000 || !c->coarse_step || c->coarse_step > (uint64_t)span ||
        !c->fine_step || c->fine_step > c->coarse_step || !c->min_contrast_span ||
        !c->max_moves || c->max_moves > 256 || !c->max_measurements || c->max_measurements > 256 ||
        !c->max_steps || c->max_steps > 1024 || !c->max_duration_ms || c->max_duration_ms > 60000 ||
        c->settle_ms > 10000 || !c->max_age_ms || c->max_age_ms > 5000) return false;
    memset(s, 0, sizeof(*s));
    s->config = *c;
    s->started_at = s->last_delivery = started_at;
    return issue(s, a, c->position_min, started_at, AF_START, false, 0);
}

static bool confident(const af_state *s) {
    return s->best_known && s->best_score >= s->config.min_peak_score &&
           s->score_max - s->score_min >= s->config.min_contrast_span;
}

void af_step(af_state *s, const af_observation *o, af_action *a) {
    if (s->phase == 3) { emit(s, a, AF_HOLD, AF_TERMINAL, true, o->sequence_id); return; }
    if (o->delivered_at < s->last_delivery || o->delivered_at < s->started_at ||
        o->acquired_at > o->delivered_at) {
        stop(s, a, AF_FAIL, AF_CLOCK, o->sequence_id); return;
    }
    s->last_delivery = o->delivered_at;
    if (s->steps >= s->config.max_steps) {
        stop(s, a, AF_FAIL, AF_STEP_LIMIT, o->sequence_id); return;
    }
    s->steps++;
    if (o->delivered_at - s->started_at >= s->config.max_duration_ms) {
        stop(s, a, AF_FAIL, AF_TIME_LIMIT, o->sequence_id); return;
    }
    if (o->cancelled) { stop(s, a, AF_HAND_BACK, AF_CANCELLED, o->sequence_id); return; }
    if (!o->supported_mode) { stop(s, a, AF_HAND_BACK, AF_MODE, o->sequence_id); return; }
    if (o->exposure_epoch != s->config.exposure_epoch) {
        stop(s, a, AF_HAND_BACK, AF_EXPOSURE, o->sequence_id); return;
    }
    if (s->sequence_seen && o->sequence_id <= s->last_sequence) {
        emit(s, a, AF_HOLD, AF_DUPLICATE, true, o->sequence_id); return;
    }
    s->sequence_seen = true;
    s->last_sequence = o->sequence_id;
    if (o->command_id_at_acquisition != s->command_id ||
        o->commanded_position_at_acquisition != s->pending_position) {
        emit(s, a, AF_HOLD, AF_COMMAND_MISMATCH, true, o->sequence_id); return;
    }
    if (o->acquired_at < s->command_issued_at ||
        o->acquired_at - s->command_issued_at < s->config.settle_ms) {
        emit(s, a, AF_HOLD, AF_EARLY, true, o->sequence_id); return;
    }
    if (o->delivered_at - o->acquired_at > s->config.max_age_ms) {
        emit(s, a, AF_HOLD, AF_STALE, true, o->sequence_id); return;
    }
    if ((o->completion_known && !o->reported_completion) ||
        (!o->completion_known && s->config.require_completion)) {
        emit(s, a, AF_HOLD, AF_WAIT_COMPLETION, true, o->sequence_id); return;
    }
    if ((o->position_known && o->reported_position != s->pending_position) ||
        (!o->position_known && s->config.require_measured_position)) {
        emit(s, a, AF_HOLD, AF_WAIT_POSITION, true, o->sequence_id); return;
    }
    if (!o->valid || !o->score_known) {
        emit(s, a, AF_HOLD, AF_INVALID, true, o->sequence_id); return;
    }
    if (s->measurements >= s->config.max_measurements) {
        stop(s, a, AF_FAIL, AF_MEASUREMENT_LIMIT, o->sequence_id); return;
    }
    s->measurements++;
    if (s->phase == 2) {
        if (!confident(s) || o->sharpness < s->config.min_peak_score ||
            (o->sharpness < s->best_score && s->best_score - o->sharpness > s->config.confirmation_drop)) {
            stop(s, a, AF_FAIL, AF_CONFIRM_FAILED, o->sequence_id); return;
        }
        stop(s, a, AF_FINISH, AF_CONFIRMED, o->sequence_id); return;
    }
    if (!s->best_known) {
        s->best_known = true;
        s->best_position = s->pending_position;
        s->best_score = s->score_min = s->score_max = o->sharpness;
    } else {
        if (o->sharpness > s->best_score) { s->best_score = o->sharpness; s->best_position = s->pending_position; }
        if (o->sharpness < s->score_min) s->score_min = o->sharpness;
        if (o->sharpness > s->score_max) s->score_max = o->sharpness;
    }
    if (s->phase == 0 && s->pending_position < s->config.position_max) {
        int64_t next = (int64_t)s->pending_position + s->config.coarse_step;
        if (next > s->config.position_max) next = s->config.position_max;
        (void)issue(s, a, next, o->delivered_at, AF_COARSE, true, o->sequence_id); return;
    }
    if (s->phase == 0) {
        if (!confident(s)) { stop(s, a, AF_FAIL, AF_LOW_CONFIDENCE, o->sequence_id); return; }
        int64_t low = (int64_t)s->best_position - s->config.coarse_step;
        int64_t high = (int64_t)s->best_position + s->config.coarse_step;
        if (low < s->config.position_min) low = s->config.position_min;
        if (high > s->config.position_max) high = s->config.position_max;
        s->fine_end = (int32_t)high;
        s->phase = 1;
        (void)issue(s, a, low, o->delivered_at, AF_FINE, true, o->sequence_id); return;
    }
    if (s->pending_position < s->fine_end) {
        int64_t next = (int64_t)s->pending_position + s->config.fine_step;
        if (next > s->fine_end) next = s->fine_end;
        (void)issue(s, a, next, o->delivered_at, AF_FINE, true, o->sequence_id); return;
    }
    s->phase = 2;
    (void)issue(s, a, s->best_position, o->delivered_at, AF_CONFIRM, true, o->sequence_id);
}

const char *af_kind_name(af_kind k) {
    static const char *const names[] = {"hold", "move_to", "move_by", "finish", "fail", "hand_back_to_stock"};
    return (unsigned)k < sizeof(names)/sizeof(names[0]) ? names[k] : "invalid";
}
const char *af_reason_name(af_reason r) {
    static const char *const names[] = {"start_search", "scan_coarse", "scan_fine", "confirm_peak", "confirmed",
        "wait_completion", "wait_position", "early_sample", "stale_sample", "acquisition_command_mismatch",
        "invalid_observation", "duplicate_sequence", "exposure_changed", "cancelled", "unsupported_mode", "clock_error",
        "observation_limit", "measurement_limit", "move_limit", "time_limit", "low_confidence", "final_verification_failed", "terminal", "bounds_error"};
    return (unsigned)r < sizeof(names)/sizeof(names[0]) ? names[r] : "invalid";
}
