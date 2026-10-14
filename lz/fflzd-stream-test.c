#include "lz/fflzd-codec.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

/* Streams are hand-authored from the token grammar in fflzd.c. */
static void split_overlap(void) {
    uint8_t output[14];
    const uint8_t first[] = {3, 'a'};
    const uint8_t second[] = {'b', 'c', 0x80};
    const uint8_t third[] = {0x39, 2, '!', '?'};
    FflzStream stream;
    fflz_stream_init(&stream, (FflzOutput){output, sizeof output});
    assert(fflz_stream_feed(&stream, (FflzInput){first, sizeof first}) == FFLZ_OK);
    assert(fflz_stream_feed(&stream, (FflzInput){second, sizeof second}) == FFLZ_OK);
    assert(fflz_stream_feed(&stream, (FflzInput){third, sizeof third}) == FFLZ_OK);
    assert(fflz_stream_finish(&stream) == FFLZ_OK);
    assert(memcmp(output, "abcabcabcabc!?", sizeof output) == 0);
}

static void rejected(FflzInput input, enum FflzStatus expected) {
    uint8_t output[8] = {0};
    FflzStream stream;
    fflz_stream_init(&stream, (FflzOutput){output, sizeof output});
    enum FflzStatus status = fflz_stream_feed(&stream, input);
    if (status == FFLZ_OK) status = fflz_stream_finish(&stream);
    assert(status == expected);
}

static void output_bound(void) {
    const uint8_t input[] = {3, 'a', 'b', 'c'};
    uint8_t output[] = {0, 0, 0xa5};
    FflzStream stream;
    fflz_stream_init(&stream, (FflzOutput){output, 2});
    assert(fflz_stream_feed(&stream, (FflzInput){input, sizeof input}) == FFLZ_OUTPUT_LIMIT);
    assert(output[2] == 0xa5);
}

static void sticky_rejection(void) {
    const uint8_t invalid[] = {0};
    const uint8_t valid[] = {1, 'a'};
    uint8_t output[] = {0xa5};
    FflzStream stream;
    fflz_stream_init(&stream, (FflzOutput){output, sizeof output});
    assert(fflz_stream_feed(&stream, (FflzInput){invalid, sizeof invalid}) == FFLZ_ZERO_TOKEN);
    assert(fflz_stream_feed(&stream, (FflzInput){valid, sizeof valid}) == FFLZ_ZERO_TOKEN);
    assert(output[0] == 0xa5);
}

int main(void) {
    const uint8_t before_start[] = {0x80, 0x11};
    const uint8_t zero_distance[] = {0x80, 0x01};
    const uint8_t zero_length[] = {1, 'a', 0x80, 0x10};
    const uint8_t partial_literal[] = {2, 'a'};
    const uint8_t partial_match[] = {1, 'a', 0x80};
    split_overlap();
    rejected((FflzInput){before_start, sizeof before_start}, FFLZ_BAD_DISTANCE);
    rejected((FflzInput){zero_distance, sizeof zero_distance}, FFLZ_ZERO_DISTANCE);
    rejected((FflzInput){zero_length, sizeof zero_length}, FFLZ_ZERO_LENGTH);
    rejected((FflzInput){partial_literal, sizeof partial_literal}, FFLZ_INCOMPLETE);
    rejected((FflzInput){partial_match, sizeof partial_match}, FFLZ_INCOMPLETE);
    output_bound();
    sticky_rejection();
    puts("stream decoder: split tokens, overlapping history, malformed input and output bounds passed");
    return 0;
}
