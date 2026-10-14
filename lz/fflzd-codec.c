#include "lz/fflzd-codec.h"
#include <string.h>

static enum FflzStatus reject(FflzStream *stream, enum FflzStatus status) {
    stream->status = status;
    return status;
}

static void finish_token(FflzStream *stream) {
    size_t last = stream->consumed + 0x14 - 1;
    if (stream->token_start / 0x4000 != last / 0x4000) stream->stats.cross_page++;
    if (stream->token_start % 0x4000 == 0) stream->stats.at_page++;
    stream->phase = FFLZ_TOKEN;
}

void fflz_stream_init(FflzStream *stream, FflzOutput output) {
    *stream = (FflzStream){ .output = output, .phase = FFLZ_TOKEN, .status = FFLZ_OK };
}

enum FflzStatus fflz_stream_feed(FflzStream *stream, FflzInput input) {
    if (stream->status != FFLZ_OK) return stream->status;
    size_t cursor = 0;
    while (cursor < input.length) {
        if (stream->phase == FFLZ_LITERAL) {
            size_t count = input.length - cursor;
            if (count > stream->pending) count = stream->pending;
            memcpy(stream->output.bytes + stream->produced, input.bytes + cursor, count);
            stream->produced += count;
            stream->consumed += count;
            stream->stats.literal_bytes += count;
            cursor += count;
            stream->pending -= count;
            if (stream->pending == 0) {
                stream->stats.literals++;
                finish_token(stream);
            }
        } else if (stream->phase == FFLZ_MATCH) {
            unsigned value = ((stream->pending & 0x7f) << 8) | input.bytes[cursor++];
            stream->consumed++;
            unsigned distance = value >> 4, length = value & 15;
            if (distance == 0) return reject(stream, FFLZ_ZERO_DISTANCE);
            if (length == 0) return reject(stream, FFLZ_ZERO_LENGTH);
            if (distance > stream->produced) return reject(stream, FFLZ_BAD_DISTANCE);
            if (length > stream->output.capacity - stream->produced) return reject(stream, FFLZ_OUTPUT_LIMIT);
            if (distance >= length) {
                memcpy(stream->output.bytes + stream->produced,
                       stream->output.bytes + stream->produced - distance, length);
                stream->produced += length;
            } else {
                for (unsigned index = 0; index < length; index++) {
                    stream->output.bytes[stream->produced] = stream->output.bytes[stream->produced - distance];
                    stream->produced++;
                }
            }
            stream->stats.matches++;
            stream->stats.length_histogram[length]++;
            if (length < 3) stream->stats.short_matches++;
            if (distance > stream->stats.max_distance) stream->stats.max_distance = distance;
            finish_token(stream);
        } else {
            stream->token_start = stream->consumed + 0x14;
            stream->pending = input.bytes[cursor++];
            stream->consumed++;
            if (stream->pending == 0) return reject(stream, FFLZ_ZERO_TOKEN);
            if (stream->pending < 0x80) {
                if (stream->pending > stream->output.capacity - stream->produced) return reject(stream, FFLZ_OUTPUT_LIMIT);
                stream->phase = FFLZ_LITERAL;
            } else {
                stream->phase = FFLZ_MATCH;
            }
        }
    }
    return FFLZ_OK;
}

enum FflzStatus fflz_stream_finish(const FflzStream *stream) {
    if (stream->status != FFLZ_OK) return stream->status;
    return stream->phase == FFLZ_TOKEN ? FFLZ_OK : FFLZ_INCOMPLETE;
}
