/* Host-only explicit-field bridge, not a camera protocol or raw struct dump. */
#include "af_kernel.h"
#include <errno.h>
#include <inttypes.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static bool unsigned_value(const char *text, uint64_t max, uint64_t *value) {
    if (!text[0]) return false;
    for (const char *p = text; *p; p++) if (*p < '0' || *p > '9') return false;
    errno = 0;
    char *end;
    uintmax_t n = strtoumax(text, &end, 10);
    if (errno || *end || n > max) return false;
    *value = (uint64_t)n;
    return true;
}
static bool signed_value(const char *text, int32_t *value) {
    const char *digits = text[0] == '-' ? text + 1 : text;
    if (!digits[0]) return false;
    for (const char *p = digits; *p; p++) if (*p < '0' || *p > '9') return false;
    errno = 0;
    char *end;
    intmax_t n = strtoimax(text, &end, 10);
    if (errno || *end || n < INT32_MIN || n > INT32_MAX) return false;
    *value = (int32_t)n;
    return true;
}
static size_t fields(char *line, char **tokens, size_t limit) {
    size_t n = 0;
    for (char *p = strtok(line, " \t\r\n"); p; p = strtok(NULL, " \t\r\n")) {
        if (n == limit) return 0;
        tokens[n++] = p;
    }
    return n;
}
static void output(const af_action *a) {
    printf("ACT1 %s %s %" PRIu32 " %u %" PRIu32 " %u %" PRId32 " %" PRIu32 " %" PRIu32 " %" PRIu32 " %u\n",
        af_kind_name(a->kind), af_reason_name(a->reason), a->command_id, (unsigned)a->trigger_known,
        a->triggering_sequence_id, (unsigned)a->target_known, a->target_position,
        a->measurements, a->moves, a->steps, (unsigned)a->terminal);
    fflush(stdout);
}
int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--help") == 0) {
        puts("fuji-af synthetic A1 C bridge: AF1 config then bounded OBS1 records on stdin; no camera adapter");
        return 0;
    }
    if (argc != 1) { fputs("unsupported arguments; synthetic stdin bridge only\n", stderr); return 1; }
    char line[1024], *t[20];
    if (!fgets(line, sizeof(line), stdin) || !strchr(line, '\n') || fields(line,t,20) != 18 || strcmp(t[0],"AF1") != 0) {
        fputs("invalid AF1 configuration framing\n",stderr); return 1;
    }
    af_config c = {0};
    uint64_t values[15];
    if (!signed_value(t[1], &c.position_min) || !signed_value(t[2], &c.position_max)) return 1;
    for (size_t i = 0; i < 15; i++) {
        uint64_t max = i == 14 ? UINT64_MAX : (i == 11 || i == 12 ? 1 : UINT32_MAX);
        if (!unsigned_value(t[i+3], max, &values[i])) { fputs("invalid bounded configuration integer\n",stderr); return 1; }
    }
    c.coarse_step=(uint32_t)values[0]; c.fine_step=(uint32_t)values[1]; c.min_peak_score=(uint32_t)values[2];
    c.min_contrast_span=(uint32_t)values[3]; c.confirmation_drop=(uint32_t)values[4]; c.settle_ms=(uint32_t)values[5];
    c.max_age_ms=(uint32_t)values[6]; c.max_moves=(uint32_t)values[7]; c.max_measurements=(uint32_t)values[8];
    c.max_steps=(uint32_t)values[9]; c.max_duration_ms=(uint32_t)values[10];
    c.require_measured_position=values[11] != 0; c.require_completion=values[12] != 0; c.exposure_epoch=(uint32_t)values[13];
    af_state state;
    af_action action;
    if (!af_init(&state, &c, values[14], &action)) { fputs("rejected synthetic configuration\n",stderr); return 1; }
    output(&action);
    for (uint32_t count = 0; count <= 1024; count++) {
        if (!fgets(line,sizeof(line),stdin)) { fputs("incomplete synthetic input stream\n",stderr); return 2; }
        if (!strchr(line,'\n') || fields(line,t,20) != 16 || strcmp(t[0],"OBS1") != 0) {
            fputs("invalid OBS1 observation framing\n",stderr); return 1;
        }
        af_observation o = {0};
        uint64_t v[15];
        for (size_t i=0; i<15; i++) {
            if (i==2 || i==6) continue;
            const bool flag = i==5 || i==7 || i==8 || i==9 || i==10 || i==13 || i==14;
            uint64_t max = flag ? 1 : (i==3 || i==4 ? UINT64_MAX : UINT32_MAX);
            if (!unsigned_value(t[i+1],max,&v[i])) { fputs("invalid bounded observation integer\n",stderr); return 1; }
        }
        if (!signed_value(t[3],&o.commanded_position_at_acquisition) || !signed_value(t[7],&o.reported_position)) return 1;
        o.sequence_id=(uint32_t)v[0]; o.command_id_at_acquisition=(uint32_t)v[1]; o.acquired_at=v[3]; o.delivered_at=v[4];
        o.position_known=v[5] != 0; o.completion_known=v[7] != 0; o.reported_completion=v[8] != 0;
        o.valid=v[9] != 0; o.score_known=v[10] != 0; o.sharpness=(uint32_t)v[11]; o.exposure_epoch=(uint32_t)v[12];
        o.cancelled=v[13] != 0; o.supported_mode=v[14] != 0;
        af_step(&state,&o,&action);
        output(&action);
        if (action.terminal) return 0;
    }
    fputs("synthetic bridge observation limit\n",stderr); return 1;
}
