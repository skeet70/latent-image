/*
 * fflzd: strict, bounded decoder for the two compressed records in the
 * X-M1 1.50 complemented payload (inferred grammar, see NOTES.md).
 * Host-only. No device access. Reads one regular file, writes one new file.
 *
 * usage: fflzd PAYLOAD RECORD_OFFSET OUT [EXPECTED_OUT_SIZE]
 *
 * Record header (5 LE words): out_size, total_len, pages, page_size(0x4000), mode(4).
 * Stream starts at +0x14 and is a sequence of tokens:
 *   T in 0x01..0x7f : literal run, T bytes follow verbatim.
 *   T in 0x80..0xff : match, 2 bytes: v = ((T & 0x7f) << 8) | next;
 *                     dist = v >> 4 (1..2047), len = v & 0xf (1..15);
 *                     copy len bytes from out[op - dist] (overlap allowed).
 *   T == 0x00, dist == 0, len == 0 : unexplained -> reject.
 * End: stream ends exactly at total_len and out == out_size, else reject.
 * Limits: input record <= 8 MiB, output <= 16 MiB and <= out_size (no margin
 * needed: any write past out_size is rejected), CPU 10 s, wall 15 s, AS 256 MiB.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <unistd.h>
#include "lz/fflzd-codec.h"

#define MAX_IN (8u << 20)
#define MAX_OUT (16u << 20)
#define PAGE 0x4000u

static void die(const char *m, long a, long b) {
    fprintf(stderr, "fflzd: REJECT %s (0x%lx, 0x%lx)\n", m, a, b);
    exit(1);
}
static void on_alarm(int s) { (void)s; static const char m[] = "fflzd: REJECT wall-clock limit\n"; if (write(2, m, sizeof m - 1)) {} _exit(3); }
static uint32_t le32(const uint8_t *p) { return p[0] | p[1] << 8 | p[2] << 16 | (uint32_t)p[3] << 24; }

int main(int argc, char **argv) {
    if (argc != 4 && argc != 5) { fprintf(stderr, "usage: fflzd PAYLOAD OFFSET OUT [EXPECTED_SIZE]\n"); return 2; }
    struct rlimit rc = {10, 10}, ra = {256u << 20, 256u << 20}, rf = {MAX_OUT, MAX_OUT};
    if (setrlimit(RLIMIT_CPU, &rc) || setrlimit(RLIMIT_AS, &ra) || setrlimit(RLIMIT_FSIZE, &rf)) die("setrlimit", errno, 0);
    signal(SIGALRM, on_alarm);
    alarm(15);

    char *end;
    errno = 0;
    unsigned long off = strtoul(argv[2], &end, 0);
    if (errno || *end) die("bad offset", 0, 0);
    long expect = -1;
    if (argc == 5) { expect = strtol(argv[4], &end, 0); if (*end || expect < 0) die("bad expected size", 0, 0); }

    int fd = open(argv[1], O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) die("open input", errno, 0);
    struct stat st;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode)) die("input not regular", 0, 0);
    if (off > (unsigned long)st.st_size || st.st_size - off < 0x14) die("offset beyond input", off, st.st_size);

    uint8_t hdr[0x14];
    if (pread(fd, hdr, 0x14, off) != 0x14) die("short header read", off, 0);
    uint32_t osz = le32(hdr), tlen = le32(hdr + 4), pages = le32(hdr + 8), psz = le32(hdr + 12), mode = le32(hdr + 16);
    if (psz != PAGE) die("page size", psz, PAGE);
    if (mode != 4) die("unsupported mode word", mode, 4);
    if (tlen < 0x14 || tlen > MAX_IN || tlen > st.st_size - off) die("total length", tlen, st.st_size - off);
    if (pages != (tlen + PAGE - 1) / PAGE) die("page count", pages, (tlen + PAGE - 1) / PAGE);
    if (osz == 0 || osz > MAX_OUT) die("output size", osz, MAX_OUT);
    if (expect >= 0 && (uint32_t)expect != osz) die("declared size != expected", osz, expect);

    uint8_t *in = malloc(tlen), *out = malloc(osz);
    if (!in || !out) die("malloc", 0, 0);
    if (pread(fd, in, tlen, off) != (ssize_t)tlen) die("short record read", tlen, 0);
    close(fd);

    FflzStream stream;
    fflz_stream_init(&stream, (FflzOutput){ out, osz });
    enum FflzStatus status = fflz_stream_feed(&stream, (FflzInput){ in + 0x14, tlen - 0x14 });
    if (status == FFLZ_OK) status = fflz_stream_finish(&stream);
    if (status != FFLZ_OK) die("invalid token stream", stream.consumed + 0x14, status);
    if (stream.produced != osz) die("output size mismatch", stream.produced, osz);

    int ofd = open(argv[3], O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0644);
    if (ofd < 0) die("create output (must not exist)", errno, 0);
    if (write(ofd, out, osz) != (ssize_t)osz || fsync(ofd) || close(ofd)) die("write output", errno, 0);

    printf("record 0x%lx: hdr out=0x%x len=0x%x pages=%u page=0x%x mode=%u\n", off, osz, tlen, pages, psz, mode);
    printf("consumed 0x%zx of 0x%x, produced 0x%zx\n", stream.consumed + 0x14, tlen, stream.produced);
    printf("literal tokens %lu (%lu bytes), match tokens %lu, max dist %lu, len<3 matches %lu\n", stream.stats.literals, stream.stats.literal_bytes, stream.stats.matches, stream.stats.max_distance, stream.stats.short_matches);
    printf("tokens straddling a 0x4000 record-offset page boundary: %lu; tokens starting on a boundary: %lu\n", stream.stats.cross_page, stream.stats.at_page);
    printf("match length histogram:");
    for (int i = 1; i < 16; i++) printf(" %d:%lu", i, stream.stats.length_histogram[i]);
    printf("\n");
    return 0;
}
