/* The recipe-bank runtime model: the stock ABI surface the bank kernel calls,
 * placed at its byte-proven 1.50 addresses by recipe-bank-model.ld.
 *
 * Media bases complete each request inside the call (the kernel posts wait 1):
 * the stored callback is invoked with the 0x71d450-mapped completion status
 * before the base returns. A completion code of 1/2/5/9/0xffffff02 is a
 * pre-dispatch refusal: the base returns 0 and allocates nothing. Any other
 * completion is dispatched: the base returns the provisional tag and the
 * worker-side release of a failed open has already run.
 *
 * The physical read model (Sol review, byte-proven): a read at or beyond the
 * file length completes with status 8 / count 0 (raw 0x25 maps to completion
 * 8); a read inside the file succeeds clipped with count = min(L, N - pos).
 *
 * Media evaluation (0x0071d66c) is called by the close worker before the
 * callback and does NOT move the media epoch. The epoch moves only at the two
 * device-event workers, the detach worker (0x0071fa9c) and the attach worker
 * (0x0071f7dc); each shim bumps the kernel's rb_media_epoch before returning.
 * The stock worker bodies behind those shims are not modeled.
 *
 * The getter/setter are the sanctioned synthetic fallback for the stock
 * 0x0061e110 / 0x00624694 slices: field-map extract/place over the three
 * 0x28-byte rows at 0x0096cc64 + 0x1c2*bank + 0x140 + 0x28*sel (the pinned
 * getter layout) plus the two documented alias properties, 0x1c6 at
 * 0x0096ce0a and 0xe8 at 0x0096ce05, which are bank-level and read/written
 * regardless of the row selector. The field table is placed in model data by
 * the case generator from reports/custom-settings.fields.json.
 */
#include <stdint.h>
#include "../menu/recipe_bank_ui.h"

#define RBM_CARD_BASE 0x00980000u
#define RBM_FILE_STRIDE 0x220u
#define RBM_TABLE_BASE 0x00970800u
#define RBM_TRIP_ADDR 0x00005150u
#define RBM_ROW_BASE 0x0096cc64u
#define RBM_ROW_STRIDE 0x1c2u
#define RBM_ROW0 0x140u
#define RBM_ROW_SIZE 0x28u
#define RBM_DEFAULTS_ROW 0x007cb804u
#define RBM_ALIAS_1C6 0x0096ce0au
#define RBM_ALIAS_E8 0x0096ce05u

typedef void (*rbm_cb)(uint32_t, uint32_t);

/* fixture and observation cells; initial bytes come from the case generator */
volatile uint32_t rbm_open_comp;
volatile uint32_t rbm_read_comp;
volatile uint32_t rbm_write_comp;
volatile uint32_t rbm_close_comp;
volatile uint32_t rbm_close_noop;
enum RbmOpenFault { RBM_OPEN_NORMAL, RBM_OPEN_ZERO_TAG };
enum RbmCloseFault {
  RBM_CLOSE_NORMAL,
  RBM_CLOSE_NOOP_WRITE,
  RBM_CLOSE_KEEP_VERIFY,
  RBM_CLOSE_LATE_VERIFY
};
volatile enum RbmOpenFault rbm_open_fault;
volatile enum RbmCloseFault rbm_close_fault;
volatile uint32_t rbm_written_file;
volatile uint32_t rbm_detach_before_stat;
volatile uint32_t rbm_trip_step;
volatile uint32_t rbm_step;
enum RbmMediaBoundary {
  RBM_BOUNDARY_NONE,
  RBM_BOUNDARY_DESTRUCTIVE_OPEN,
  RBM_BOUNDARY_WRITE,
  RBM_BOUNDARY_CLOSE,
  RBM_BOUNDARY_DISCOVERY_STAT
};
volatile enum RbmMediaBoundary rbm_swap_boundary;
typedef struct {
  const uint8_t *bytes;
} RbmCardSource;
volatile RbmCardSource rbm_swap_card;
volatile uint32_t rbm_swap_b_present;
volatile uint32_t rbm_swap_b_size;
volatile uint32_t rbm_swap_a_size;
volatile uint32_t rbm_wrote;
volatile uint32_t rbm_write_short;
volatile uint32_t rbm_trip_armed;
volatile uint32_t rbm_a_present;
volatile uint32_t rbm_a_size;
volatile uint32_t rbm_b_present;
volatile uint32_t rbm_b_size;
volatile uint32_t rbm_h_used[4];
volatile uint32_t rbm_h_file[4];
volatile uint32_t rbm_h_cursor[4];
volatile uint32_t rbm_h_mode[4];
volatile uint32_t rbm_field_count;

/* the media layer's own tables sit at the stock addresses the kernel's
 * ownership check and the worker's release chain read */
#define RBM_TAG_TABLE ((volatile uint32_t *)0x009101c0u)
#define RBM_MAP_TABLE ((volatile uint32_t *)0x00910140u)
#define RBM_INDEX_TABLE ((volatile uint32_t *)0x00910150u)
#define RBM_LOW_TABLE ((volatile uint32_t *)0x00910160u)


/* one persistence boundary; the trip store fires when the boundary number
 * matches the fixture's trip step. Steps: 1 write start, 2 mid-partial write,
 * 3 after write before close, 4 after close before verify read,
 * 5 during verify. */
__attribute__((naked, noinline)) void rbm_interrupt(uint32_t address,
                                                   uint32_t step)
{
  __asm__ volatile("str r1, [r0]\n\tbx lr");
}

__attribute__((noinline)) uint32_t rbm_boundary(void)
{
  rbm_step = rbm_step + 1u;
  if (rbm_trip_armed != 0u && rbm_trip_step != 0u && rbm_step == rbm_trip_step) {
    rbm_interrupt(RBM_TRIP_ADDR, rbm_step);
  }
  return rbm_step;
}

static void rbm_copy(uint8_t *dst, const uint8_t *src, uint32_t n)
{
  uint32_t i;
  for (i = 0u; i < n; i++) {
    dst[i] = src[i];
  }
}

uint32_t rbm_detach_event(uint32_t a0, uint32_t a1)
{
  rb_media_epoch_bump();
  return a0 + a1;
}

uint32_t rbm_attach_event(uint32_t a0, uint32_t a1)
{
  rb_media_epoch_bump();
  return a0 + a1;
}

uint32_t rbm_media_eval(uint32_t a0, uint32_t a1)
{
  return a0 + a1;
}

static void rbm_run_swap(void)
{
  const uint8_t *src = (const uint8_t *)(RBM_CARD_BASE + 2u * RBM_FILE_STRIDE);
  rbm_copy((uint8_t *)RBM_CARD_BASE, src, RBM_FILE_STRIDE);
  rbm_a_present = 1u;
  rbm_a_size = rbm_swap_a_size;
  rbm_b_present = 0u;
  rbm_b_size = 0u;
}

static void rbm_swap_at(enum RbmMediaBoundary boundary)
{
  if (rbm_swap_boundary != boundary) {
    return;
  }
  rbm_swap_boundary = RBM_BOUNDARY_NONE;
  rbm_detach_event(0u, 0u);
  rbm_attach_event(0u, 0u);
  if (rbm_swap_card.bytes != 0u) {
    rbm_copy((uint8_t *)RBM_CARD_BASE, rbm_swap_card.bytes, 2u * RBM_FILE_STRIDE);
    rbm_a_present = 1u;
    rbm_a_size = rbm_swap_a_size;
  } else {
    rbm_run_swap();
  }
  rbm_b_present = rbm_swap_b_present;
  rbm_b_size = rbm_swap_b_size;
}

static uint32_t rbm_alloc_slot(void)
{
  uint32_t i;
  for (i = 0u; i < 4u; i++) {
    if (rbm_h_used[i] == 0u) {
      return i + 1u;
    }
  }
  return 0u;
}

static uint32_t rbm_slot(uint32_t handle)
{
  return (handle >= 1u && handle <= 4u) ? (handle - 1u) : 0xffffffffu;
}

static uint32_t rbm_file_of_path(const uint8_t *path)
{
  static const char NAME_A[] = "RECIPEA.BIN";
  static const char NAME_B[] = "RECIPEB.BIN";
  uint32_t i;
  uint32_t match_a = 1u;
  uint32_t match_b = 1u;
  for (i = 0u; i < 11u; i++) {
    if (path[3u + i] != (uint8_t)NAME_A[i]) {
      match_a = 0u;
    }
    if (path[3u + i] != (uint8_t)NAME_B[i]) {
      match_b = 0u;
    }
  }
  if (match_a != 0u) {
    return 0u;
  }
  return (match_b != 0u) ? 1u : 0xffffffffu;
}

static uint8_t *rbm_file_bytes(uint32_t file)
{
  return (uint8_t *)(RBM_CARD_BASE + file * RBM_FILE_STRIDE);
}

static volatile uint32_t *rbm_file_present(uint32_t file)
{
  return (file == 0u) ? &rbm_a_present : &rbm_b_present;
}

static volatile uint32_t *rbm_file_size(uint32_t file)
{
  return (file == 0u) ? &rbm_a_size : &rbm_b_size;
}

static void rbm_release_worker(uint32_t handle)
{
  uint32_t s = rbm_slot(handle);
  if (s != 0xffffffffu) {
    RBM_TAG_TABLE[s] = 0u;
    RBM_MAP_TABLE[s] = 0u;
    RBM_INDEX_TABLE[s] = 0u;
    RBM_LOW_TABLE[s] = 0u;
    rbm_h_used[s] = 0u;
    rbm_h_file[s] = 0u;
    rbm_h_cursor[s] = 0u;
    rbm_h_mode[s] = 0u;
  }
}

uint32_t rbm_tag_release(uint32_t tag)
{
  uint32_t s = rbm_slot(tag);
  if (s != 0xffffffffu) {
    RBM_TAG_TABLE[s] = 0u;
    rbm_h_used[s] = 0u;
  }
  return 0u;
}

uint32_t rbm_resolve(uint32_t tag)
{
  return rbm_slot(tag);
}

uint32_t rbm_mapping_delete(uint32_t tag)
{
  uint32_t s = rbm_slot(tag);
  if (s != 0xffffffffu) {
    RBM_MAP_TABLE[s] = 0u;
  }
  return 0u;
}

uint32_t rbm_low_close(uint32_t index)
{
  if (index < 4u) {
    RBM_LOW_TABLE[index] = 0u;
  }
  return 0u;
}

uint32_t rbm_index_free(uint32_t index)
{
  if (index < 4u) {
    RBM_INDEX_TABLE[index] = 0u;
  }
  return 0u;
}

uint32_t rbm_open(rbm_cb cb, const uint8_t *path, uint32_t mode, uint32_t guard,
                  uint32_t wait, uint32_t variant)
{
  uint32_t file;
  uint32_t handle;
  uint32_t slot;
  (void)guard;
  (void)wait;
  (void)variant;
  RbMediaIdentity identity = rb_io_request_identity();
  if (mode == 1u) {
    rbm_swap_at(RBM_BOUNDARY_DESTRUCTIVE_OPEN);
  }
  if (!rb_media_worker_allows(cb, identity)) {
    cb(2u, 0u);
    return 1u;
  }
  file = rbm_file_of_path(path);
  handle = rbm_alloc_slot();
  if (handle == 0u || file == 0xffffffffu) {
    cb(6u, 0u);
    return 0u;
  }
  if (rbm_open_comp != 0u) {
    if (rbm_open_comp == 1u || rbm_open_comp == 2u || rbm_open_comp == 5u ||
        rbm_open_comp == 9u || rbm_open_comp == 0xffffff02u) {
      cb(rbm_open_comp, 0u);
      return 0u;
    }
    if (rbm_open_comp == 6u) {
      cb(6u, 0u);
      return 1u;
    }
    cb(rbm_open_comp, 0u);
    return 1u;
  }
  if (mode == 0u && *rbm_file_present(file) == 0u) {
    cb(6u, 0u);
    return 1u;
  }
  slot = rbm_slot(handle);
  rbm_h_used[slot] = 1u;
  rbm_h_file[slot] = file;
  rbm_h_cursor[slot] = 0u;
  rbm_h_mode[slot] = mode;
  if (mode != 0u) {
    *rbm_file_present(file) = 1u;
    *rbm_file_size(file) = 0u;
  }
  RBM_TAG_TABLE[slot] = handle;
  RBM_MAP_TABLE[slot] = 1u;
  RBM_INDEX_TABLE[slot] = 1u;
  RBM_LOW_TABLE[slot] = 1u;
  if (rbm_open_fault == RBM_OPEN_ZERO_TAG) {
    rbm_open_fault = RBM_OPEN_NORMAL;
    cb(0u, handle);
    return 0u;
  }
  cb(0u, handle);
  return handle;
}

uint32_t rbm_read(rbm_cb cb, uint32_t handle, uint32_t len, uint8_t *buf,
                  uint32_t wait, uint32_t variant)
{
  uint32_t slot = rbm_slot(handle);
  uint32_t file;
  uint32_t pos;
  uint32_t size;
  uint32_t count;
  const uint8_t *src;
  (void)wait;
  (void)variant;
  if (slot == 0xffffffffu || rbm_h_used[slot] == 0u) {
    cb(9u, 0u);
    return 0u;
  }
  if (rbm_read_comp != 0u) {
    if (rbm_read_comp == 1u || rbm_read_comp == 2u || rbm_read_comp == 5u ||
        rbm_read_comp == 9u || rbm_read_comp == 0xffffff02u) {
      cb(rbm_read_comp, 0u);
      return 0u;
    }
    cb(rbm_read_comp, 0u);
    return 1u;
  }
  file = rbm_h_file[slot];
  pos = rbm_h_cursor[slot];
  size = *rbm_file_size(file);
  if (pos >= size) {
    cb(8u, 0u);
    return 1u;
  }
  count = size - pos;
  if (count > len) {
    count = len;
  }
  src = rbm_file_bytes(file) + pos;
  rbm_copy(buf, src, count);
  rbm_h_cursor[slot] = pos + count;
  if (rbm_wrote != 0u) {
    rbm_wrote = 0u;
    rbm_boundary();
  }
  cb(0u, count);
  return 1u;
}

/* the fault variant of the write: copies the first rbm_write_short bytes and
 * completes short, which the kernel maps to a full-card refusal */
__attribute__((noinline)) static uint32_t
rbm_write_short_case(rbm_cb cb, uint32_t handle, uint32_t len,
                     const uint8_t *src)
{
  uint32_t slot = rbm_slot(handle);
  uint32_t n = rbm_write_short;
  uint32_t file = rbm_h_file[slot];
  uint32_t pos = rbm_h_cursor[slot];
  uint32_t i;
  uint8_t *dst = rbm_file_bytes(file);
  rbm_step = 0u;
  rbm_trip_armed = 1u;
  rbm_boundary();
  for (i = 0u; i < n; i++) {
    dst[pos + i] = src[i];
  }
  rbm_h_cursor[slot] = pos + n;
  if (pos + n > *rbm_file_size(file)) {
    *rbm_file_size(file) = pos + n;
  }
  rbm_boundary();
  rbm_wrote = 1u;
  rbm_written_file = file + 1u;
  cb(0u, n);
  (void)len;
  return 1u;
}

__attribute__((noinline)) static uint32_t
rbm_write_full_case(rbm_cb cb, uint32_t handle, uint32_t len, const uint8_t *src)
{
  uint32_t slot = rbm_slot(handle);
  uint32_t file = rbm_h_file[slot];
  uint32_t pos = rbm_h_cursor[slot];
  uint32_t i;
  uint8_t *dst = rbm_file_bytes(file);
  rbm_step = 0u;
  rbm_trip_armed = 1u;
  rbm_boundary();
  for (i = 0u; i < len; i++) {
    if (i != 0u && i == (len >> 1)) {
      rbm_boundary();
    }
    dst[pos + i] = src[i];
  }
  rbm_h_cursor[slot] = pos + len;
  if (pos + len > *rbm_file_size(file)) {
    *rbm_file_size(file) = pos + len;
  }
  rbm_boundary();
  rbm_wrote = 1u;
  rbm_written_file = file + 1u;
  cb(0u, len);
  return 1u;
}

uint32_t rbm_write(rbm_cb cb, uint32_t handle, uint32_t len, const uint8_t *src,
                   uint32_t wait, uint32_t variant)
{
  uint32_t slot = rbm_slot(handle);
  (void)wait;
  (void)variant;
  RbMediaIdentity identity = rb_io_request_identity();
  rbm_swap_at(RBM_BOUNDARY_WRITE);
  if (!rb_media_worker_allows(cb, identity)) {
    cb(2u, 0u);
    return 1u;
  }
  if (slot == 0xffffffffu || rbm_h_used[slot] == 0u) {
    cb(9u, 0u);
    return 0u;
  }
  if (rbm_write_comp != 0u) {
    uint32_t c = rbm_write_comp;
    cb(c, 0u);
    return (c == 1u || c == 2u || c == 5u || c == 9u || c == 0xffffff02u) ? 0u : 1u;
  }
  if (rbm_write_short != 0u && rbm_write_short < len) {
    return rbm_write_short_case(cb, handle, len, src);
  }
  return rbm_write_full_case(cb, handle, len, src);
}

uint32_t rbm_stat(uint32_t handle, uint8_t *stat)
{
  uint32_t slot = rbm_slot(handle);
  uint32_t i;
  uint32_t size;
  rbm_swap_at(RBM_BOUNDARY_DISCOVERY_STAT);
  if (rbm_detach_before_stat != 0u) {
    rbm_detach_before_stat = 0u;
    rbm_detach_event(0u, 0u);
  }
  if (slot == 0xffffffffu || rbm_h_used[slot] == 0u) {
    return 9u;
  }
  size = *rbm_file_size(rbm_h_file[slot]);
  for (i = 0u; i < 64u; i++) {
    stat[i] = 0u;
  }
  stat[52] = (uint8_t)size;
  stat[53] = (uint8_t)(size >> 8);
  stat[54] = (uint8_t)(size >> 16);
  stat[55] = (uint8_t)(size >> 24);
  return 0u;
}

__attribute__((noinline)) static uint32_t rbm_close_shape(rbm_cb cb,
                                                          uint32_t handle)
{
  uint32_t slot = rbm_slot(handle);
  uint32_t verify = rbm_h_mode[slot] == 0u &&
                    rbm_written_file == rbm_h_file[slot] + 1u;
  if (rbm_close_fault == RBM_CLOSE_LATE_VERIFY && verify != 0u) {
    rbm_close_fault = RBM_CLOSE_NORMAL;
    rbm_release_worker(handle);
    return 1u;
  }
  if (rbm_close_noop != 0u ||
      (rbm_close_fault == RBM_CLOSE_NOOP_WRITE && rbm_h_mode[slot] == 1u)) {
    if (rbm_h_mode[slot] == 1u) {
      rbm_close_fault = RBM_CLOSE_NORMAL;
    }
    cb(0u, 0u);
    return 2u;
  }
  if (rbm_close_fault == RBM_CLOSE_KEEP_VERIFY && verify != 0u) {
    rbm_close_fault = RBM_CLOSE_NORMAL;
    cb(0u, 0u);
    return 3u;
  }
  return 0u;
}

uint32_t rbm_close(rbm_cb cb, uint32_t handle, uint32_t z0, uint32_t z1,
                   uint32_t wait, uint32_t variant)
{
  uint32_t slot = rbm_slot(handle);
  uint32_t shape;
  (void)z0;
  (void)z1;
  (void)wait;
  (void)variant;
  RbMediaIdentity identity = rb_io_request_identity();
  rbm_swap_at(RBM_BOUNDARY_CLOSE);
  if (!rb_media_worker_allows(cb, identity)) {
    rbm_release_worker(handle);
    cb(2u, 0u);
    return 1u;
  }
  if (slot == 0xffffffffu || rbm_h_used[slot] == 0u) {
    cb(9u, 0u);
    return 0u;
  }
  shape = rbm_close_shape(cb, handle);
  if (shape == 1u || shape == 3u) {
    return 1u;
  }
  if (shape == 2u) {
    return 0u;
  }
  if (rbm_close_comp != 0u) {
    if (rbm_close_comp == 1u || rbm_close_comp == 2u || rbm_close_comp == 5u ||
        rbm_close_comp == 9u || rbm_close_comp == 0xffffff02u) {
      /* a pre-dispatch shape releases nothing; the tag stays allocated and
       * the handle is deferred for the next open to flush through the worker */
      cb(rbm_close_comp, 0u);
      return 0u;
    }
    cb(rbm_close_comp, 0u);
    rbm_release_worker(handle);
    return 1u;
  }
  rbm_media_eval(0u, 0u);
  cb(0u, 0u);
  rbm_release_worker(handle);
  rbm_boundary();
  return 1u;
}

static volatile uint32_t *rbm_field(uint32_t prop)
{
  volatile uint32_t *entry = (volatile uint32_t *)RBM_TABLE_BASE;
  uint32_t i;
  uint32_t n = rbm_field_count;
  for (i = 0u; i < n; i++) {
    if (entry[5u * i] == prop) {
      return entry + 5u * i;
    }
  }
  return 0u;
}

static uint8_t *rbm_field_addr(uint32_t prop, uint32_t bank, uint32_t use_defaults,
                               uint32_t sel, uint32_t off)
{
  if (prop == 0x1c6u) {
    return (uint8_t *)(RBM_ALIAS_1C6 - 0x04u);
  }
  if (prop == 0xe8u) {
    return (uint8_t *)(RBM_ALIAS_E8 - 0x04u);
  }
  if (use_defaults != 0u) {
    return (uint8_t *)(RBM_DEFAULTS_ROW + RBM_ROW_SIZE * sel + off);
  }
  return (uint8_t *)(RBM_ROW_BASE + RBM_ROW_STRIDE * bank + RBM_ROW0 +
                     RBM_ROW_SIZE * sel + off);
}

uint32_t rbm_get(uint32_t prop, uint32_t bank, uint32_t use_defaults,
                 uint32_t sel)
{
  volatile uint32_t *f = rbm_field(prop);
  uint32_t off;
  uint32_t shift;
  uint32_t bytes;
  uint32_t mask;
  uint32_t v;
  uint8_t *base;
  if (f == 0u) {
    return 0u;
  }
  off = f[1];
  shift = f[2];
  bytes = f[3];
  mask = f[4];
  base = rbm_field_addr(prop, bank, use_defaults, sel, off);
  v = base[0];
  if (bytes == 2u) {
    v |= (uint32_t)base[1] << 8;
  }
  return (v & mask) >> shift;
}

uint32_t rbm_set(uint32_t prop, uint32_t code, uint32_t zero, uint32_t sel)
{
  volatile uint32_t *f = rbm_field(prop);
  uint32_t off;
  uint32_t shift;
  uint32_t bytes;
  uint32_t mask;
  uint32_t v;
  uint32_t placed;
  uint8_t *base;
  (void)zero;
  if (f == 0u) {
    return 0u;
  }
  off = f[1];
  shift = f[2];
  bytes = f[3];
  mask = f[4];
  base = rbm_field_addr(prop, 0u, 0u, sel, off);
  v = base[0];
  placed = (code << shift) & mask;
  if (bytes == 2u) {
    v |= (uint32_t)base[1] << 8;
    v = (v & ~(mask << shift)) | placed;
    base[0] = (uint8_t)v;
    base[1] = (uint8_t)(v >> 8);
  } else {
    v = (v & ~mask) | placed;
    base[0] = (uint8_t)v;
  }
  return 1u;
}

#ifndef RBM_MEDIA_ONLY
/* case entries for contracts a single kernel call cannot express */
extern uint32_t rb_load(void);

uint32_t rbm_seq_stage_cancel(uint32_t slot)
{
  RbMediaIdentity identity = rb_media_sample();
  uint32_t status = rb_stage_load(slot, identity);
  if (status != 0u) {
    return status;
  }
  rb_stage_cancel();
  return 0u;
}

uint32_t rbm_seq_stage_field_cancel(uint32_t slot, uint32_t prop, uint32_t code)
{
  RbMediaIdentity identity = rb_media_sample();
  uint32_t status = rb_stage_load(slot, identity);
  if (status != 0u) {
    return status;
  }
  status = rb_stage_field(prop, code);
  if (status != 0u) {
    return status;
  }
  rb_stage_cancel();
  return 0u;
}

uint32_t rbm_seq_stage_field_commit(uint32_t slot, uint32_t prop, uint32_t code,
                                    const uint8_t *name12)
{
  RbMediaIdentity identity = rb_media_sample();
  uint32_t status = rb_stage_load(slot, identity);
  if (status != 0u) {
    return status;
  }
  status = rb_stage_field(prop, code);
  if (status != 0u) {
    return status;
  }
  return rb_stage_commit(slot, name12);
}

uint32_t rbm_seq_stage_field_recall(uint32_t slot, uint32_t prop, uint32_t code)
{
  uint32_t status = rbm_seq_stage_field_commit(slot, prop, code, 0);
  if (status != 0u) {
    return status;
  }
  return rb_recall(slot, rb_media_sample());
}

uint32_t rbm_seq_load_recall(uint32_t slot)
{
  uint32_t status = rb_load();
  if (status != 0u) {
    return status;
  }
  return rb_recall(slot, rb_media_sample());
}

uint32_t rbm_seq_stage_roundtrip(uint32_t slot, const uint8_t *name12)
{
  RbMediaIdentity identity = rb_media_sample();
  uint32_t status = rb_stage_load(slot, identity);
  if (status != 0u) {
    return status;
  }
  return rb_stage_commit(slot, name12);
}

uint32_t rbm_seq_load_readback(uint32_t slot, uint8_t *name, uint8_t *payload)
{
  uint32_t status = rb_load();
  if (status != RB_OK) {
    return status;
  }
  return rb_readback(slot, name, payload, rb_media_sample());
}

uint32_t rbm_seq_load_all_readback(uint8_t *names, uint8_t *payloads)
{
  uint32_t status = rb_load();
  if (status != RB_OK) {
    return status;
  }
  for (uint32_t slot = 0u; slot < RB_BANK_SLOTS; slot++) {
    status = rb_readback(slot, names + slot * 26u, payloads + slot * 40u,
                         rb_media_sample());
    if (status != RB_OK) {
      return status;
    }
  }
  return RB_OK;
}

uint32_t rbm_seq_load_twice(uint32_t *first_status)
{
  *first_status = rb_load();
  return rbm_seq_load_readback(0u, (uint8_t *)0x009a0020u,
                             (uint8_t *)0x009a0040u);
}

uint32_t rbm_seq_save_twice(uint32_t slot, uint32_t *first_status)
{
  *first_status = rb_save_current(slot, rb_media_sample());
  return rb_save_current(slot, rb_media_sample());
}

uint32_t rbm_seq_read_after_deferred(void)
{
  for (uint32_t i = 0u; i < 8u; i++) {
    rbm_close_noop = 1u;
    uint32_t status = rb_load();
    rbm_close_noop = 0u;
    if (status != RB_OK) {
      return status;
    }
    status = rb_readback(1u, (uint8_t *)0x009a0000u,
                         (uint8_t *)0x009a0020u, rb_media_sample());
    if (status != RB_OK) {
      return status;
    }
  }
  return RB_OK;
}

/* F9 menu-navigation flows (RECIPE-UI-BEHAVIOR-01). Every step sets the
 * modal-75 cells the interposer reads and enters the patched cell, so the
 * flow goes through rb_bank_interpose exactly as the stock dispatcher
 * delivers it. The cursor cell holds the row index into the frame's values
 * array. */

#define RBM_UI_FRAME 0x009571d0u
#define RBM_UI_CURSOR 0x009571d8u
#define RBM_UI_STATE 0x0146761eu
#define RBM_UI_EVENT 0x0146777au

static uint32_t rbm_ui_enter(uint32_t frame, uint32_t cursor, uint32_t state,
                             uint32_t event)
{
  *(volatile uint32_t *)RBM_UI_FRAME = frame;
  *(volatile uint32_t *)RBM_UI_CURSOR = cursor;
  *(volatile uint16_t *)RBM_UI_STATE = (uint16_t)state;
  *(volatile uint16_t *)RBM_UI_EVENT = (uint16_t)event;
  return rb_bank_interpose();
}

uint32_t rbm_seq_ui(uint32_t frame, uint32_t cursor, uint32_t state,
                    uint32_t event)
{
  return rbm_ui_enter(frame, cursor, state, event);
}

/* the per-item confirm flow: push the field list from the edit screen's row,
 * select the field row, take the leaving edge to the item dialog and resolve
 * it with OK or CANCEL */
uint32_t rbm_seq_ui_item(uint32_t edit, uint32_t row, uint32_t field,
                         uint32_t value, uint32_t dialog, uint32_t ok)
{
  uint32_t r;
  r = rbm_ui_enter(edit, row, 0u, 8u);
  (void)r;
  r = rbm_ui_enter(field, value, 0u, 8u);
  (void)r;
  r = rbm_ui_enter(field, 0u, 0u, 13u);
  (void)r;
  return rbm_ui_enter(dialog, ok != 0u ? 0x80u : 0x81u, 0u, 8u);
}

/* the whole-edit confirm flow: the edit screen's leaving edge opens the
 * commit dialog and the dialog resolves it */
uint32_t rbm_seq_ui_whole(uint32_t edit, uint32_t dialog, uint32_t ok)
{
  uint32_t r = rbm_ui_enter(edit, 0u, 0u, 13u);
  (void)r;
  return rbm_ui_enter(dialog, ok != 0u ? 0x80u : 0x81u, 0u, 8u);
}

/* the Save Current and Reset confirmations: the submenu's verb row opens its
 * confirm dialog and the dialog resolves it */
uint32_t rbm_seq_ui_confirm(uint32_t sub, uint32_t verb, uint32_t dialog,
                            uint32_t ok)
{
  uint32_t r = rbm_ui_enter(sub, verb, 0u, 8u);
  (void)r;
  return rbm_ui_enter(dialog, ok != 0u ? 0x80u : 0x81u, 0u, 8u);
}

/* the rename editor's confirm and cancel entries at modal state 0x1e */
uint32_t rbm_seq_ui_rename(uint32_t editor, uint32_t confirm)
{
  return rbm_ui_enter(editor, confirm != 0u ? 0x2au : 0x2bu, 0x1eu,
                      confirm != 0u ? 8u : 13u);
}
#endif
