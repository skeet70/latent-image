#ifndef FUJI_AF_FFLZD_CODEC_H
#define FUJI_AF_FFLZD_CODEC_H
#include <stddef.h>
#include <stdint.h>

typedef struct { const uint8_t *bytes; size_t length; } FflzInput;
typedef struct { uint8_t *bytes; size_t capacity; } FflzOutput;
enum FflzStatus { FFLZ_OK, FFLZ_ZERO_TOKEN, FFLZ_ZERO_DISTANCE, FFLZ_ZERO_LENGTH,
                  FFLZ_BAD_DISTANCE, FFLZ_OUTPUT_LIMIT, FFLZ_INCOMPLETE };
enum FflzPhase { FFLZ_TOKEN, FFLZ_LITERAL, FFLZ_MATCH };
typedef struct {
    unsigned long literals, literal_bytes, matches, short_matches, max_distance;
    unsigned long cross_page, at_page, length_histogram[16];
} FflzStats;
typedef struct {
    FflzOutput output;
    size_t produced, consumed, token_start;
    enum FflzPhase phase;
    enum FflzStatus status;
    uint8_t pending;
    FflzStats stats;
} FflzStream;

void fflz_stream_init(FflzStream *stream, FflzOutput output);
enum FflzStatus fflz_stream_feed(FflzStream *stream, FflzInput input);
enum FflzStatus fflz_stream_finish(const FflzStream *stream);
#endif
