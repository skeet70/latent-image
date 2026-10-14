/* The editable custom-recipe bank's storage kernel (X-M1 1.50, candidate,
 * non-installable). It owns the SD bank files and the record work the menu
 * hook (menu/recipe_bank_hook.c) asks for.
 *
 * Placement: linked with the menu arms into one blob carried at a plain-extent
 * gap (rebuild-update replace-plain). Slot 0 is the stock C record and is
 * never written here: recall is refused at the C dial position, the snapshot
 * only reads settings, and staged edits live in this kernel's own memory.
 * Bank-0 row 2 at 0x0096cdf4 is not staging memory: pinned setter bytes show
 * it aliases live settings (property 0x116 writes 0x0096ce0a, which the
 * ordinary getter for 0x1c6 reads), so no stock setter is called with
 * selector 2 and nothing in this file writes that record.
 *
 * Field walk: RB_FIELDS and RB_EDIT_FIELDS are generated from the
 * authoritative payload field map (reports/custom-settings.fields.json) by
 * //host/firmware:emit-recipe-bank-fields, so this kernel, the host oracle and
 * the tests read one table: the 83 walked rows in apply order (the two
 * halfword properties 0x1cb/0x1cc at +0x00/+0x02 first, then the byte fields
 * of +0x04..+0x27) and the nine recipe-edit rows. The bits no row covers come
 * from the same tool as RB_UNBOUND_MASK. Property 0xe9 at +0x0a bits 5-7 is
 * bound to no walk row: the record's own mode field is written by the mode
 * path and by the C-copy routine's explicit write, and a remembered payload
 * preserves those bits.
 *
 * Recall is the stock apply and recompute path, not a packed-byte poke: a
 * stored code that the property's conversion table carries resolves to its row
 * through the same table walk the menu's current-row resolver uses, and the
 * stock commit dispatcher 0x00d4fc74 then runs that property's own arm, its
 * processing and menu consumers (the film cell at 0x009568b8, the tone and
 * pipeline notifications 0x0063c51c/0x00641438) and the shared change notify.
 * A field whose code no row names, the halfwords included, is written through
 * the stock write entry 0x0061c91c, the entry every commit arm uses for its
 * own field. One pass then reads every field back through the stock current-
 * value reader and re-applies any field another arm rewrote (image quality's
 * arm writes 0x1d, bracketing's writes 0x1b1); a field that still does not
 * read back its stored code answers RB_APPLY_REFUSED and the caller must not
 * trust that field.
 *
 * File requests go through the media-task base functions with wait 1 and
 * variant 0, which block until the media task has called the request's
 * callback, and the callback's status and handle or count are the operation's
 * results. Requests are serialized: one is completed before the next is
 * posted. Every request carries the media epoch sampled before it, and a
 * destructive open, the write-back and the publication are each bound to it:
 * a moved epoch refuses the request before dispatch and, after a completed
 * callback, drops the handle into the deferred list and reports
 * RB_MEDIA_CHANGED without publishing. Nothing is released in the caller
 * thread: the close worker's chain (0x711768 -> 0x71f1d4 -> 0x71eff4) is the
 * only release, an un-released handle is deferred and completed by a
 * re-dispatched close inside the media queue, and the tag allocation table
 * decides ownership. A zero dispatch tag does not prove that no object was
 * opened: a nonzero callback handle is deferred on that path too, and only a
 * zero tag with no handle leaves no closable identity to act on.
 */
#include <stdint.h>

#include "recipe_bank_ui.h"

/* stock entry points (1.50, image 4 unless noted) */
#define RB_GETTER 0x0061e110u /* (prop, bank, use_defaults, slot) -> code */
#define RB_CURRENT 0x0061c93c /* (prop, bank): the current slot's value */
#define RB_WRITE 0x0061c91c /* (prop, value, 0): the menu write entry */
#define RB_COMMIT 0x00d4fc74u /* image 5: (prop, row, 0, 0) commit dispatcher */
#define RB_CONVERT 0x0061d514 /* (prop, value, column, return) table walk */
#define RB_TABLE_MISS 0x0089edc8u /* walker's not-found flag */
#define RB_LIVE_SLOT 0x006439e0u /* () -> 0 at C (slot-0 record), 1 ordinary */
#define RB_BASE_OPEN 0x0071c2b0u /* (cb, path, mode, guard, wait, variant) */
#define RB_BASE_READ 0x0071be74u /* (cb, handle, length, buffer, wait, variant) */
#define RB_BASE_WRITE 0x0071bfe0u /* (cb, handle, length, source, wait, variant) */
#define RB_BASE_CLOSE 0x0071c134u /* (cb, handle, 0, 0, wait, variant) */
#define RB_FILE_STAT 0x0071ce84u  /* (handle, statbuf) -> 0; size word at +52 */
#define RB_CARD_STATE 0x0071ddf0u /* (0) -> 0 ready, 1/5/2 not ready */
#define RB_VOL_LETTER 0x0071f708u /* () -> active volume letter, 0 when none */
#define RB_TASK_ID 0x00723cb8u    /* () -> calling task id */
#define RB_TABLE_PTRS 0x0081b09cu /* + 4*property -> conversion table pointer */
#define RB_MODE_CLASS 0x00951564u /* == 3 while the dial is at C */
#define RB_DEFAULTS_ROW 0x007cb804u

#define RB_HEADER 32u
#define RB_PAYLOAD 0x28u
#define RB_NAME 26u
#define RB_NAME_UNITS 12u
#define RB_BLOB (RB_BANK_SLOTS * (RB_PAYLOAD + RB_NAME))
#define RB_FILE_BYTES 512u
#define RB_FORMAT_VERSION 1u
#define RB_FIRMWARE_TAG 150u

#define RB_FILE_A 0u
#define RB_FILE_B 1u

struct rb_field {
  uint16_t prop;
  uint8_t off;
  uint8_t shift;
  uint8_t bytes;
  uint16_t mask;
};

/* The walked payload field table and the nine recipe-edit rows are generated
 * from the authoritative map, so this kernel, the host oracle
 * (host/FujiAF/RecipeBank.hs) and the tests read the same rows. */
#include "recipe_bank_fields.inc"

static uint8_t rb_resident[RB_FILE_BYTES];
static uint8_t rb_staging[RB_FILE_BYTES + 1u];
static uint8_t rb_verify[RB_FILE_BYTES + 1u];
static uint8_t rb_stage_payload[RB_PAYLOAD];
static uint8_t rb_path[2][16];
static volatile uint32_t rb_io_status;
static volatile uint32_t rb_io_value;
static volatile uint32_t rb_io_done;
static uint32_t rb_loaded;
static uint32_t rb_current_gen;
static uint32_t rb_load_outcome = RB_NO_BANK;
static uint32_t rb_stage_slot = 0xffffffffu;

static void rb_memset(uint8_t *dst, uint8_t v, uint32_t n)
{
  uint32_t i;
  for (i = 0u; i < n; i++) {
    dst[i] = v;
  }
}

static void rb_memcpy(uint8_t *dst, const uint8_t *src, uint32_t n)
{
  uint32_t i;
  for (i = 0u; i < n; i++) {
    dst[i] = src[i];
  }
}

static uint32_t rb_bytes_equal(const uint8_t *a, const uint8_t *b, uint32_t n)
{
  uint32_t i;
  for (i = 0u; i < n; i++) {
    if (a[i] != b[i]) {
      return 0u;
    }
  }
  return 1u;
}

static void rb_io_cb(uint32_t status, uint32_t value)
{
  rb_io_status = status;
  rb_io_value = value;
  rb_io_done = 1u;
}

/* the callback status is 0x71d450's completion code: 3 prepare (raw 0x20),
 * 0xe commit (raw 0xd), 8 the raw 0x24 class, 6 missing file (raw 7/8) */
static uint32_t rb_map_completion(uint32_t code)
{
  if (code == 0u) {
    return RB_OK;
  }
  if (code == 3u || code == 0x0eu) {
    return RB_FULL;
  }
  if (code == 8u) {
    return RB_READ_ONLY;
  }
  if (code == 6u) {
    return RB_MISSING;
  }
  return RB_IO;
}

/* the tag allocation table 0x71ef18 hands out and the worker's release chain
 * clears; a tag still present after a close completed was not released */
static uint32_t rb_tag_owned(uint32_t tag)
{
  const uint32_t *table = (const uint32_t *)(uintptr_t)0x009101c0u;
  uint32_t i;
  if (tag == 0u) {
    return 0u;
  }
  for (i = 0u; i < 16u; i++) {
    if (table[i] == tag) {
      return 1u;
    }
  }
  return 0u;
}

static uint32_t rb_close_quiet(uint32_t handle);

/* a close that leaves its tag allocated released nothing, and the low-level
 * release would race the media worker's shared open-file counter (0x703f94's
 * unprotected update at 0x93a914), so the release is deferred until a
 * re-dispatched close completes inside the media queue. The list is sized to
 * the stock tag pool (16, 0x71ef18), so every deferred handle is tracked. */
static uint32_t rb_pending_close[16];
static uint32_t rb_pending_count;
static uint32_t rb_close_deferred_flag;

static void rb_defer_close(uint32_t handle)
{
  uint32_t i;
  if (handle == 0u) {
    return;
  }
  for (i = 0u; i < rb_pending_count; i++) {
    if (rb_pending_close[i] == handle) {
      return;
    }
  }
  if (rb_pending_count < 16u) {
    rb_pending_close[rb_pending_count] = handle;
    rb_pending_count++;
  }
}

static void rb_flush_pending(void)
{
  uint32_t kept = 0u;
  uint32_t i;
  for (i = 0u; i < rb_pending_count; i++) {
    if (rb_tag_owned(rb_pending_close[i]) == 0u) {
      /* a late completion released it after our await gave up */
      continue;
    }
    (void)rb_close_quiet(rb_pending_close[i]);
    if (rb_close_deferred_flag != 0u) {
      rb_pending_close[kept] = rb_pending_close[i];
      kept++;
    }
  }
  rb_pending_count = kept;
}

static uint32_t rb_card_ready(void)
{
  uint32_t (*card)(uint32_t) = (uint32_t (*)(uint32_t))RB_CARD_STATE;
  uint32_t state = card(0u);
  return (state == 0u) ? RB_OK : RB_NOT_READY;
}

/* waiting on the media task blocks on its semaphores and would deadlock from
 * inside the media task or task 0 */
static uint32_t rb_task_ok(void)
{
  uint32_t (*task)(void) = (uint32_t (*)(void))RB_TASK_ID;
  uint32_t id = task();
  return (id == 0x1cu || id == 0u) ? 0u : 1u;
}

static void rb_io_begin(void)
{
  rb_io_status = 0xffffffffu;
  rb_io_value = 0u;
  rb_io_done = 0u;
}

static uint32_t rb_await(void) { return (rb_io_done != 0u) ? RB_OK : RB_IO; }

/* the path letter comes from the volume accessor the auto-act loader uses
 * (0xed1de0 writes 0x71f708's result over its template letter) */
static uint32_t rb_build_path(uint32_t which)
{
  uint32_t (*letter_fn)(void) = (uint32_t (*)(void))RB_VOL_LETTER;
  static const char NAME_A[] = "RECIPEA.BIN";
  static const char NAME_B[] = "RECIPEB.BIN";
  const char *name = (which == RB_FILE_A) ? NAME_A : NAME_B;
  uint32_t i;
  uint32_t l = letter_fn();
  if (l == 0u) {
    return RB_NOT_READY;
  }
  rb_path[which][0] = (uint8_t)l;
  rb_path[which][1] = ':';
  rb_path[which][2] = '\\';
  for (i = 0u; i < 11u; i++) {
    rb_path[which][3u + i] = (uint8_t)name[i];
  }
  rb_path[which][14] = 0u;
  return RB_OK;
}

/* The close worker's release chain runs before its callback, so a completed
 * close whose tag is still allocated released nothing (every pre-dispatch
 * shape, including the state-1 no-op, leaves it); that release is deferred to
 * a re-dispatched close. A close that never completed leaves the tag owned
 * the same way. */
static uint32_t rb_close_quiet(uint32_t handle)
{
  uint32_t (*close_fn)(void *, uint32_t, uint32_t, uint32_t, uint32_t,
                       uint32_t) =
      (uint32_t (*)(void *, uint32_t, uint32_t, uint32_t, uint32_t,
                    uint32_t))RB_BASE_CLOSE;
  uint32_t mapped;
  rb_close_deferred_flag = 0u;
  if (rb_task_ok() == 0u) {
    rb_close_deferred_flag = 1u;
    return RB_TASK_REFUSED;
  }
  rb_io_begin();
  (void)close_fn((void *)rb_io_cb, handle, 0u, 0u, 1u, 0u);
  if (rb_await() != RB_OK) {
    rb_close_deferred_flag = 1u;
    return RB_IO;
  }
  mapped = rb_map_completion(rb_io_status);
  if (rb_tag_owned(handle) != 0u) {
    rb_close_deferred_flag = 1u;
  }
  return (mapped == RB_OK && rb_io_status != 0u) ? RB_IO : mapped;
}

/* Posts one open and binds it to the caller's media identity: a moved epoch
 * refuses before dispatch, and a completion whose epoch moved drops the handle
 * into the deferred list instead of returning it. A zero dispatch tag does not
 * prove that no object was opened, so a nonzero callback handle is deferred on
 * that path as well. */
static uint32_t rb_open(uint32_t which, uint32_t mode, uint32_t epoch,
                        uint32_t *handle_out)
{
  uint32_t (*open_fn)(void *, const uint8_t *, uint32_t, uint32_t, uint32_t,
                      uint32_t) =
      (uint32_t (*)(void *, const uint8_t *, uint32_t, uint32_t, uint32_t,
                    uint32_t))RB_BASE_OPEN;
  uint32_t status;
  uint32_t tag;
  if (rb_task_ok() == 0u) {
    return RB_TASK_REFUSED;
  }
  rb_flush_pending();
  status = rb_build_path(which);
  if (status != RB_OK) {
    return status;
  }
  status = rb_card_ready();
  if (status != RB_OK) {
    return status;
  }
  if (rb_media_epoch() != epoch) {
    return RB_MEDIA_CHANGED;
  }
  rb_io_begin();
  tag = open_fn((void *)rb_io_cb, rb_path[which], mode, 1u, 1u, 0u);
  if (rb_await() != RB_OK) {
    /* no callback: a dispatched request may still complete late, and without
     * the callback's handle the request has no closable identity here */
    return RB_IO;
  }
  if (rb_media_epoch() != epoch) {
    /* the identity moved between the request and its completion */
    if (rb_io_value != 0u) {
      rb_defer_close(rb_io_value);
    }
    return RB_MEDIA_CHANGED;
  }
  if (rb_io_status != 0u) {
    /* a nonzero return means the request dispatched and the open worker
     * released the tag on failure (0x71f3cc-0x71f3e8); a zero return means
     * the request was refused before dispatch */
    uint32_t mapped = rb_map_completion(rb_io_status);
    return (mapped == RB_OK) ? RB_IO : mapped;
  }
  if (tag == 0u || rb_io_value == 0u) {
    if (rb_io_value != 0u) {
      rb_defer_close(rb_io_value);
    }
    return RB_IO;
  }
  *handle_out = rb_io_value;
  return RB_OK;
}

static uint32_t rb_read_count(uint32_t handle, uint8_t *buf, uint32_t len,
                              uint32_t *count_out)
{
  uint32_t (*read_fn)(void *, uint32_t, uint32_t, uint8_t *, uint32_t,
                      uint32_t) =
      (uint32_t (*)(void *, uint32_t, uint32_t, uint8_t *, uint32_t,
                    uint32_t))RB_BASE_READ;
  if (rb_task_ok() == 0u) {
    return RB_TASK_REFUSED;
  }
  rb_io_begin();
  (void)read_fn((void *)rb_io_cb, handle, len, buf, 1u, 0u);
  if (rb_await() != RB_OK) {
    return RB_IO;
  }
  if (rb_io_status != 0u) {
    uint32_t mapped = rb_map_completion(rb_io_status);
    return (mapped == RB_OK) ? RB_IO : mapped;
  }
  *count_out = rb_io_value;
  return RB_OK;
}

static uint32_t rb_read(uint32_t handle, uint8_t *buf, uint32_t len)
{
  uint32_t count = 0u;
  uint32_t status = rb_read_count(handle, buf, len, &count);
  if (status != RB_OK) {
    return status;
  }
  return (count == len) ? RB_OK : RB_IO;
}

static uint32_t rb_write(uint32_t handle, const uint8_t *src, uint32_t len)
{
  uint32_t (*write_fn)(void *, uint32_t, uint32_t, const uint8_t *, uint32_t,
                       uint32_t) =
      (uint32_t (*)(void *, uint32_t, uint32_t, const uint8_t *, uint32_t,
                    uint32_t))RB_BASE_WRITE;
  if (rb_task_ok() == 0u) {
    return RB_TASK_REFUSED;
  }
  rb_io_begin();
  (void)write_fn((void *)rb_io_cb, handle, len, src, 1u, 0u);
  if (rb_await() != RB_OK) {
    return RB_IO;
  }
  if (rb_io_status != 0u) {
    uint32_t mapped = rb_map_completion(rb_io_status);
    return (mapped == RB_OK) ? RB_IO : mapped;
  }
  return (rb_io_value == len) ? RB_OK : RB_FULL;
}

/* bank file codec ------------------------------------------------------- */

static uint16_t rb_u16(const uint8_t *p)
{
  return (uint16_t)((uint16_t)p[0] | ((uint16_t)p[1] << 8));
}

static uint32_t rb_u32(const uint8_t *p)
{
  return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) |
         ((uint32_t)p[3] << 24);
}

static void rb_put_u16(uint8_t *p, uint16_t v)
{
  p[0] = (uint8_t)(v & 0xffu);
  p[1] = (uint8_t)(v >> 8);
}

static void rb_put_u32(uint8_t *p, uint32_t v)
{
  p[0] = (uint8_t)(v & 0xffu);
  p[1] = (uint8_t)((v >> 8) & 0xffu);
  p[2] = (uint8_t)((v >> 16) & 0xffu);
  p[3] = (uint8_t)((v >> 24) & 0xffu);
}

/* the container main-range convention over the whole file with the checksum
 * field zeroed: the complement of the sum of the eight-bit complemented bytes
 * (E-CHECKSUM), so generation and the slot mask are covered */
static uint32_t rb_checksum_file(const uint8_t *file)
{
  uint32_t sum = 0u;
  uint32_t i;
  for (i = 0u; i < RB_FILE_BYTES; i++) {
    uint8_t b = (i >= 0x18u && i < 0x1cu) ? 0u : file[i];
    sum += (uint32_t)(uint8_t)~b;
  }
  return ~sum;
}

static uint32_t rb_generation(const uint8_t *file) { return rb_u32(file + 0x10); }
static uint16_t rb_slot_mask(const uint8_t *file) { return rb_u16(file + 0x1cu); }
static uint8_t *rb_payload_at(uint8_t *file, uint32_t slot)
{
  return file + RB_HEADER + slot * RB_PAYLOAD;
}
static uint8_t *rb_name_at(uint8_t *file, uint32_t slot)
{
  return file + RB_HEADER + RB_BANK_SLOTS * RB_PAYLOAD + slot * RB_NAME;
}

/* a name is 12 UTF-16LE units of printable ASCII, a zero unit, zero fill; the
 * terminator unit is bytes 24-25 */
static uint32_t rb_name_ok(const uint8_t *name)
{
  uint32_t k;
  uint32_t ended = 0u;
  if (name[24] != 0u || name[25] != 0u) {
    return 0u;
  }
  for (k = 0u; k < RB_NAME_UNITS; k++) {
    uint8_t lo = name[2u * k];
    uint8_t hi = name[2u * k + 1u];
    if (ended != 0u) {
      if (lo != 0u || hi != 0u) {
        return 0u;
      }
    } else if (lo == 0u && hi == 0u) {
      ended = 1u;
    } else if (hi != 0u || lo < 0x20u || lo > 0x7eu) {
      return 0u;
    }
  }
  return 1u;
}

static uint32_t rb_extract(const uint8_t *payload, const struct rb_field *f)
{
  uint32_t v = payload[f->off];
  if (f->bytes == 2u) {
    v |= (uint32_t)payload[f->off + 1] << 8;
  }
  return (v & f->mask) >> f->shift;
}

static void rb_place(uint8_t *payload, const struct rb_field *f, uint32_t code)
{
  uint32_t v = payload[f->off];
  uint32_t placed = (code << f->shift) & f->mask;
  if (f->bytes == 2u) {
    v |= (uint32_t)payload[f->off + 1] << 8;
    v = (v & ~((uint32_t)f->mask << f->shift)) | placed;
    payload[f->off] = (uint8_t)(v & 0xffu);
    payload[f->off + 1] = (uint8_t)((v >> 8) & 0xffu);
  } else {
    v = (v & ~(uint32_t)f->mask) | placed;
    payload[f->off] = (uint8_t)v;
  }
}

/* the property's conversion-table pointer word: 0 means the field carries no
 * table domain (the halfword properties 0x1cb/0x1cc among them) */
static uint32_t rb_table_ptr(uint32_t prop)
{
  if (prop > 0x2ffu) {
    return 0u;
  }
  return *(const uint32_t *)(uintptr_t)(RB_TABLE_PTRS + 4u * prop);
}

/* the stock conversion walk (0x0061d514 -> 0x00631a00): the first row whose
 * column `column` equals `value`, returned through `want`. The walker's
 * not-found flag (0x0089edc8) reads 0 only when a row matched. */
static uint32_t rb_convert(uint32_t prop, uint32_t value, uint32_t column,
                           uint32_t want)
{
  uint32_t (*convert)(uint32_t, uint32_t, uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t, uint32_t, uint32_t))RB_CONVERT;
  return convert(prop, value, column, want);
}

/* the row a stored code commits through, and whether a row carried it: the
 * same direction the menu's current-row resolver 0x00d588b8 uses */
static uint32_t rb_code_row(uint32_t prop, uint32_t code, uint32_t *found)
{
  volatile uint32_t *miss = (volatile uint32_t *)(uintptr_t)RB_TABLE_MISS;
  uint32_t row;
  *miss = 0u;
  row = rb_convert(prop, code, 1u, 0u);
  *found = (*miss == 0u) ? 1u : 0u;
  return row;
}

/* a code the table walk does not carry is refused; a null pointer word means
 * the field carries no table domain and every code is admissible */
static uint32_t rb_code_ok(uint32_t prop, uint32_t code)
{
  uint32_t found;
  if (rb_table_ptr(prop) == 0u) {
    return 1u;
  }
  (void)rb_code_row(prop, code, &found);
  return found;
}

/* one field of a recall through the stock path: a code the conversion table
 * carries commits through the stock commit dispatcher for that row, which runs
 * the property's own arm, its processing and menu consumers and the shared
 * notify; every other code, the halfwords included, goes through the stock
 * write entry the shared arm uses for its field */
static void rb_apply_field(const struct rb_field *f, uint32_t code)
{
  uint32_t (*write)(uint32_t, uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t, uint32_t))RB_WRITE;
  uint32_t (*commit)(uint32_t, uint32_t, uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t, uint32_t, uint32_t))RB_COMMIT;
  uint32_t found = 0u;
  uint32_t row = rb_code_row(f->prop, code, &found);
  if (found != 0u) {
    (void)commit(f->prop, row, 0u, 0u);
    return;
  }
  (void)write(f->prop, code, 0u);
}

/* the current value of a property through the stock reader, which resolves the
 * slot through 0x006439e0: the ordinary block away from the C position */
static uint32_t rb_current(uint32_t prop)
{
  uint32_t (*current)(uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t))RB_CURRENT;
  return current(prop, 0u);
}

static uint32_t rb_codes_ok(const uint8_t *payload)
{
  uint32_t f;
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    if (rb_code_ok(RB_FIELDS[f].prop, rb_extract(payload, &RB_FIELDS[f])) == 0u) {
      return 0u;
    }
  }
  return 1u;
}

/* the bits no walked row and no bound-but-not-walked row carries, generated
 * with the table: a file must hold them zero, and a remembered payload drops
 * them. +0x0a bits 5-7 carry property 0xe9, the record's own mode field, which
 * no walk carries, so a payload preserves them. */
static uint32_t rb_unbound_ok(const uint8_t *payload)
{
  uint32_t i;
  for (i = 0u; i < RB_PAYLOAD; i++) {
    if ((payload[i] & RB_UNBOUND_MASK[i]) != 0u) {
      return 0u;
    }
  }
  return 1u;
}

static void rb_payload_normalize(uint8_t *payload)
{
  uint32_t i;
  for (i = 0u; i < RB_PAYLOAD; i++) {
    payload[i] = (uint8_t)(payload[i] & (uint8_t)~RB_UNBOUND_MASK[i]);
  }
}

/* a file that fails any check here applies nothing */
static uint32_t rb_file_valid(const uint8_t *file)
{
  uint32_t slot;
  if (rb_u16(file + 0x04u) != RB_FORMAT_VERSION ||
      rb_u16(file + 0x06u) != RB_HEADER ||
      rb_u16(file + 0x08u) != RB_FIRMWARE_TAG ||
      rb_u16(file + 0x0au) != RB_BANK_SLOTS || rb_u16(file + 0x0cu) != RB_NAME_UNITS ||
      rb_u16(file + 0x0eu) != RB_PAYLOAD || rb_u32(file + 0x14u) != RB_BLOB ||
      rb_u16(file + 0x1eu) != 0u) {
    return 0u;
  }
  if (file[0] != 'F' || file[1] != 'J' || file[2] != 'R' || file[3] != 'B') {
    return 0u;
  }
  if ((uint16_t)(rb_slot_mask(file) & ~(uint16_t)((1u << RB_BANK_SLOTS) - 1u)) != 0u) {
    return 0u;
  }
  if (rb_checksum_file(file) != rb_u32(file + 0x18u)) {
    return 0u;
  }
  for (slot = 0u; slot < RB_FILE_BYTES - RB_HEADER - RB_BLOB; slot++) {
    if (file[RB_HEADER + RB_BLOB + slot] != 0u) {
      return 0u;
    }
  }
  for (slot = 0u; slot < RB_BANK_SLOTS; slot++) {
    if (rb_name_ok(rb_name_at((uint8_t *)file, slot)) == 0u) {
      return 0u;
    }
    if (rb_codes_ok(rb_payload_at((uint8_t *)file, slot)) == 0u) {
      return 0u;
    }
    if (rb_unbound_ok(rb_payload_at((uint8_t *)file, slot)) == 0u) {
      return 0u;
    }
  }
  return 1u;
}

/* load precedence: the valid file with the higher generation, a tie goes to
 * A. The write target is the file not holding the winner, and A when neither
 * is valid, so a write that dies leaves the previous bank loadable. */
static uint32_t rb_pick_current(uint32_t gen_a, uint32_t a_valid, uint32_t gen_b,
                                uint32_t b_valid)
{
  if (a_valid != 0u && (b_valid == 0u || gen_a >= gen_b)) {
    return RB_FILE_A;
  }
  if (b_valid != 0u) {
    return RB_FILE_B;
  }
  return 0xffu;
}

static void rb_header_fill(uint8_t *file, uint32_t generation, uint16_t mask)
{
  rb_memset(file, 0u, RB_HEADER);
  file[0] = 'F';
  file[1] = 'J';
  file[2] = 'R';
  file[3] = 'B';
  rb_put_u16(file + 0x04u, RB_FORMAT_VERSION);
  rb_put_u16(file + 0x06u, (uint16_t)RB_HEADER);
  rb_put_u16(file + 0x08u, RB_FIRMWARE_TAG);
  rb_put_u16(file + 0x0au, (uint16_t)RB_BANK_SLOTS);
  rb_put_u16(file + 0x0cu, (uint16_t)RB_NAME_UNITS);
  rb_put_u16(file + 0x0eu, (uint16_t)RB_PAYLOAD);
  rb_put_u32(file + 0x10u, generation);
  rb_put_u32(file + 0x14u, (uint32_t)RB_BLOB);
  rb_put_u16(file + 0x1cu, mask);
  rb_put_u32(file + 0x18u, rb_checksum_file(file));
  rb_memset(file + RB_HEADER + RB_BLOB, 0u, RB_FILE_BYTES - RB_HEADER - RB_BLOB);
}

/* load --------------------------------------------------------------- */

/* Reads one file into `out`. The loader's size query (0x71ce84, size word at
 * +52) runs before any read, so an empty or wrong-length generation is
 * excluded without a read at EOF (raw 0x25 would map to completion 8). A
 * positively missing file or a fully read malformed file is not a candidate;
 * an operational failure is returned so no target selection or publication
 * runs on it. */
static uint32_t rb_scan_file(uint32_t which, uint32_t epoch, uint8_t *out,
                             uint32_t *gen_out, uint32_t *valid_out,
                             uint32_t *seen_out)
{
  uint32_t (*stat_fn)(uint32_t, uint8_t *) =
      (uint32_t (*)(uint32_t, uint8_t *))RB_FILE_STAT;
  uint32_t handle = 0u;
  uint32_t status;
  uint32_t close_status;
  uint32_t count = 0u;
  uint8_t stat[128];
  *valid_out = 0u;
  status = rb_open(which, 0u, epoch, &handle);
  if (status != RB_OK) {
    return (status == RB_MISSING) ? RB_OK : status;
  }
  *seen_out = 1u;
  status = stat_fn(handle, stat);
  if (status != 0u) {
    status = (rb_map_completion(status) == RB_OK) ? RB_IO : rb_map_completion(status);
  } else if (rb_u32(stat + 52) != RB_FILE_BYTES) {
    /* empty, short or overlong: a malformed candidate */
    status = RB_INVALID;
  } else {
    status = rb_read_count(handle, out, RB_FILE_BYTES, &count);
    if (status == RB_OK && count != RB_FILE_BYTES) {
      status = RB_INVALID;
    }
  }
  close_status = rb_close_quiet(handle);
  if (rb_close_deferred_flag != 0u) {
    rb_defer_close(handle);
  }
  if (close_status != RB_OK) {
    return close_status;
  }
  if (status == RB_INVALID) {
    return RB_OK;
  }
  if (status != RB_OK) {
    return status;
  }
  if (rb_file_valid(out) != 0u) {
    *valid_out = 1u;
    *gen_out = rb_generation(out);
  }
  return RB_OK;
}

/* Discovery only: nothing resident is published until both scans complete and
 * the media epoch witness is unchanged across them, and each scan's open is
 * posted only while the identity holds. The winner is computed from this scan
 * alone. RB_NO_BANK (no file at all) and RB_INVALID (bytes seen, no valid
 * file) both publish the empty bank and are benign for save and reset. */
uint32_t rb_load(void)
{
  uint32_t gen_a = 0u;
  uint32_t gen_b = 0u;
  uint32_t a_valid = 0u;
  uint32_t b_valid = 0u;
  uint32_t seen = 0u;
  uint32_t epoch = rb_media_epoch();
  uint32_t status;

  status = rb_scan_file(RB_FILE_A, epoch, rb_staging, &gen_a, &a_valid, &seen);
  if (status != RB_OK) {
    rb_load_outcome = status;
    return status;
  }
  status = rb_scan_file(RB_FILE_B, epoch, rb_verify, &gen_b, &b_valid, &seen);
  if (status != RB_OK) {
    rb_load_outcome = status;
    return status;
  }
  if (rb_media_epoch() != epoch) {
    rb_load_outcome = RB_MEDIA_CHANGED;
    return RB_MEDIA_CHANGED;
  }
  rb_loaded = 0u;
  rb_current_gen = 0u;
  if (a_valid != 0u && (b_valid == 0u || gen_a >= gen_b)) {
    rb_memcpy(rb_resident, rb_staging, RB_FILE_BYTES);
    rb_current_gen = gen_a;
    rb_loaded = 1u;
    rb_load_outcome = RB_OK;
    return RB_OK;
  }
  if (b_valid != 0u) {
    rb_memcpy(rb_resident, rb_verify, RB_FILE_BYTES);
    rb_current_gen = gen_b;
    rb_loaded = 1u;
    rb_load_outcome = RB_OK;
    return RB_OK;
  }
  rb_memset(rb_resident, 0u, RB_FILE_BYTES);
  rb_load_outcome = (seen != 0u) ? RB_INVALID : RB_NO_BANK;
  return rb_load_outcome;
}

/* two-generation write of rb_staging, whose header and blob the caller has
 * filled. rb_resident is untouched until the read back is byte-equal to the
 * image we wrote; there are no retries. The scan, the destructive open, the
 * write, the verify read and the publication are all bound to the epoch
 * sampled here: a request posted after a move refuses before dispatch and a
 * completed request whose epoch moved drops its handle and reports
 * RB_MEDIA_CHANGED without publishing. */
static uint32_t rb_write_back(uint32_t generation, uint16_t mask)
{
  uint32_t gen_a = 0u;
  uint32_t gen_b = 0u;
  uint32_t a_valid = 0u;
  uint32_t b_valid = 0u;
  uint32_t seen = 0u;
  uint32_t current;
  uint32_t target;
  uint32_t handle = 0u;
  uint32_t close_status;
  uint32_t status;
  uint32_t epoch;

  if (generation == 0u) {
    return RB_GEN_EXHAUST;
  }
  rb_header_fill(rb_staging, generation, mask);
  epoch = rb_media_epoch();
  status = rb_scan_file(RB_FILE_A, epoch, rb_verify, &gen_a, &a_valid, &seen);
  if (status != RB_OK) {
    return status;
  }
  status = rb_scan_file(RB_FILE_B, epoch, rb_verify, &gen_b, &b_valid, &seen);
  if (status != RB_OK) {
    return status;
  }
  if (rb_media_epoch() != epoch) {
    return RB_MEDIA_CHANGED;
  }
  current = rb_pick_current(gen_a, a_valid, gen_b, b_valid);
  target = (current == RB_FILE_A) ? RB_FILE_B : RB_FILE_A;

  status = rb_open(target, 1u, epoch, &handle);
  if (status != RB_OK) {
    return status;
  }
  status = rb_write(handle, rb_staging, RB_FILE_BYTES);
  close_status = rb_close_quiet(handle);
  if (rb_close_deferred_flag != 0u) {
    rb_defer_close(handle);
  }
  if (close_status != RB_OK && status == RB_OK) {
    status = RB_FULL;
  }
  if (status != RB_OK) {
    return status;
  }
  if (rb_media_epoch() != epoch) {
    return RB_MEDIA_CHANGED;
  }

  status = rb_open(target, 0u, epoch, &handle);
  if (status != RB_OK) {
    return status;
  }
  status = rb_read(handle, rb_verify, RB_FILE_BYTES);
  close_status = rb_close_quiet(handle);
  if (rb_close_deferred_flag != 0u) {
    rb_defer_close(handle);
  }
  if (close_status != RB_OK && status == RB_OK) {
    status = RB_IO;
  }
  if (status != RB_OK) {
    return status;
  }
  if (rb_bytes_equal(rb_verify, rb_staging, RB_FILE_BYTES) == 0u) {
    return RB_FULL;
  }
  if (rb_media_epoch() != epoch) {
    return RB_MEDIA_CHANGED;
  }
  rb_memcpy(rb_resident, rb_staging, RB_FILE_BYTES);
  rb_loaded = 1u;
  rb_current_gen = generation;
  return RB_OK;
}

/* record work ------------------------------------------------------- */

static uint8_t rb_stage_name[RB_NAME];

/* the snapshot walk: the real getter reads every bound field from the source
 * slot (the FUN_006439e0 result, 1 for the ordinary block away from C) into the
 * staging payload. CUSTOM SET's walk 0x00630b14 is not used because its tail
 * stores the destination through 0x72b3d0, which rejects the scratch record. */
static void rb_walk_payload_from(uint32_t src_sel, uint8_t *payload)
{
  uint32_t (*getter)(uint32_t, uint32_t, uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t, uint32_t, uint32_t))RB_GETTER;
  uint32_t f;
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    const struct rb_field *field = &RB_FIELDS[f];
    rb_place(payload, field, getter(field->prop, 0u, 0u, src_sel));
  }
}

static void rb_stage_image(void)
{
  rb_memcpy(rb_staging, rb_resident, RB_FILE_BYTES);
}

uint32_t rb_readback(uint32_t slot, uint8_t *name_out, uint8_t *payload_out)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  status = rb_load();
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  rb_memcpy(name_out, rb_name_at(rb_resident, slot), RB_NAME);
  rb_memcpy(payload_out, rb_payload_at(rb_resident, slot), RB_PAYLOAD);
  return RB_OK;
}

uint32_t rb_save_current(uint32_t slot, const uint8_t *name12)
{
  uint32_t (*live)(void) = (uint32_t (*)(void))RB_LIVE_SLOT;
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  if (rb_name_ok(name12) == 0u) {
    return RB_RANGE;
  }
  status = rb_load();
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  rb_stage_image();
  rb_memset(rb_payload_at(rb_staging, slot), 0u, RB_PAYLOAD);
  rb_walk_payload_from(live(), rb_payload_at(rb_staging, slot));
  rb_payload_normalize(rb_payload_at(rb_staging, slot));
  rb_memcpy(rb_name_at(rb_staging, slot), name12, RB_NAME);
  return rb_write_back(rb_current_gen + 1u,
                       (uint16_t)(rb_slot_mask(rb_resident) |
                                  (uint16_t)(1u << slot)));
}

uint32_t rb_reset(uint32_t slot)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  status = rb_load();
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  rb_stage_image();
  rb_memset(rb_payload_at(rb_staging, slot), 0u, RB_PAYLOAD);
  rb_memset(rb_name_at(rb_staging, slot), 0u, RB_NAME);
  return rb_write_back(rb_current_gen + 1u,
                       (uint16_t)(rb_slot_mask(rb_resident) &
                                  (uint16_t) ~(1u << slot)));
}

uint32_t rb_recall(uint32_t slot)
{
  const uint8_t *payload;
  uint32_t refused = 0u;
  uint32_t f;
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  /* no bank operation writes the C record: at C the apply target resolves to
   * the slot-0 record, so recall is refused rather than applied */
  if (*(volatile uint32_t *)(uintptr_t)RB_MODE_CLASS == 3u) {
    return RB_AT_C;
  }
  status = rb_load();
  if (status != RB_OK) {
    return (status == RB_NO_BANK) ? RB_UNSET : status;
  }
  if ((rb_slot_mask(rb_resident) & (uint16_t)(1u << slot)) == 0u) {
    return RB_UNSET;
  }
  if (rb_codes_ok(rb_payload_at(rb_resident, slot)) == 0u ||
      rb_unbound_ok(rb_payload_at(rb_resident, slot)) == 0u) {
    return RB_INVALID;
  }
  payload = rb_payload_at(rb_resident, slot);
  /* the stock apply and recompute pass: every walked field goes through its
   * own stock commit, so the processing and menu consumers the menu commits
   * run are run here too */
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    rb_apply_field(&RB_FIELDS[f], rb_extract(payload, &RB_FIELDS[f]));
  }
  /* one assert pass: the stored code is the recall's contract, and another
   * field's arm writes some fields (image quality's arm writes 0x1d,
   * bracketing's writes 0x1b1), so a field that no longer reads back its code
   * is applied once more through the same path */
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    uint32_t code = rb_extract(payload, &RB_FIELDS[f]);
    if (rb_current(RB_FIELDS[f].prop) == code) {
      continue;
    }
    rb_apply_field(&RB_FIELDS[f], code);
    if (rb_current(RB_FIELDS[f].prop) != code) {
      refused = 1u;
    }
  }
  return (refused != 0u) ? RB_APPLY_REFUSED : RB_OK;
}

/* Staged edit works over this kernel's own payload buffer and touches no
 * stock record or live setting: a staged field change is a validated bit
 * placement, and only commit or recall writes anywhere. */
uint32_t rb_stage_load(uint32_t slot)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  status = rb_load();
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  if ((rb_slot_mask(rb_resident) & (uint16_t)(1u << slot)) != 0u) {
    rb_memcpy(rb_stage_payload, rb_payload_at(rb_resident, slot), RB_PAYLOAD);
    rb_memcpy(rb_stage_name, rb_name_at(rb_resident, slot), RB_NAME);
  } else {
    /* an unset slot starts from the stock per-slot defaults row */
    rb_memcpy(rb_stage_payload, (const uint8_t *)RB_DEFAULTS_ROW, RB_PAYLOAD);
    rb_memset(rb_stage_name, 0u, RB_NAME);
  }
  rb_stage_slot = slot;
  return RB_OK;
}

uint32_t rb_stage_field(uint32_t prop, uint32_t code)
{
  uint32_t f;
  for (f = 0u; f < RB_EDIT_FIELD_COUNT; f++) {
    if (RB_EDIT_FIELDS[f].prop == prop) {
      if (rb_code_ok(prop, code) == 0u) {
        return RB_RANGE;
      }
      rb_place(rb_stage_payload, &RB_EDIT_FIELDS[f], code);
      return RB_OK;
    }
  }
  return RB_RANGE;
}

uint32_t rb_stage_read(uint32_t prop)
{
  uint32_t f;
  for (f = 0u; f < RB_EDIT_FIELD_COUNT; f++) {
    if (RB_EDIT_FIELDS[f].prop == prop) {
      return rb_extract(rb_stage_payload, &RB_EDIT_FIELDS[f]);
    }
  }
  return 0xffffffffu;
}

void rb_stage_name_set(const uint8_t *name12)
{
  if (rb_name_ok(name12) != 0u) {
    rb_memcpy(rb_stage_name, name12, RB_NAME);
  }
}

/* A name-only generation write; a staged session for this same slot follows
 * the rename in its name field once the write has committed. */
uint32_t rb_name_commit(uint32_t slot, const uint8_t *name26)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  if (rb_name_ok(name26) == 0u) {
    return RB_RANGE;
  }
  status = rb_load();
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  if ((rb_slot_mask(rb_resident) & (uint16_t)(1u << slot)) == 0u) {
    return RB_UNSET;
  }
  rb_stage_image();
  rb_memcpy(rb_name_at(rb_staging, slot), name26, RB_NAME);
  status = rb_write_back(rb_current_gen + 1u, rb_slot_mask(rb_resident));
  if (status == RB_OK && rb_stage_slot == slot) {
    rb_memcpy(rb_stage_name, name26, RB_NAME);
  }
  return status;
}

uint32_t rb_stage_commit(uint32_t slot, const uint8_t *name12)
{
  uint32_t status;
  const uint8_t *name = (name12 != 0u) ? name12 : rb_stage_name;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  if (rb_name_ok(name) == 0u) {
    return RB_RANGE;
  }
  if (rb_codes_ok(rb_stage_payload) == 0u || rb_unbound_ok(rb_stage_payload) == 0u) {
    return RB_INVALID;
  }
  status = rb_load();
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  rb_stage_image();
  rb_memcpy(rb_payload_at(rb_staging, slot), rb_stage_payload, RB_PAYLOAD);
  rb_memcpy(rb_name_at(rb_staging, slot), name, RB_NAME);
  status = rb_write_back(rb_current_gen + 1u,
                         (uint16_t)(rb_slot_mask(rb_resident) |
                                    (uint16_t)(1u << slot)));
  if (status == RB_OK) {
    rb_stage_slot = 0xffffffffu;
  }
  return status;
}

void rb_stage_cancel(void)
{
  rb_memset(rb_stage_name, 0u, RB_NAME);
  rb_memset(rb_stage_payload, 0u, RB_PAYLOAD);
  rb_stage_slot = 0xffffffffu;
}

/* resident-only name read */
uint32_t rb_name_resident(uint32_t slot, uint8_t *name26)
{
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  if (rb_loaded == 0u) {
    rb_memset(name26, 0u, RB_NAME);
    return RB_UNSET;
  }
  rb_memcpy(name26, rb_name_at(rb_resident, slot), RB_NAME);
  return RB_OK;
}

/* The availability probe's read-only verdict for one slot: whether recall
 * (op 0) or save (op 1) would be attempted. The only request a call may make
 * is the read-only load a nonzero reload asks for. */
uint32_t rb_meta_op_state(enum rb_bank_op op, uint32_t slot, uint32_t reload)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  if (rb_task_ok() == 0u) {
    return RB_TASK_REFUSED;
  }
  if (reload != 0u) {
    status = rb_load();
    if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
      return status;
    }
  } else if (rb_load_outcome != RB_OK && rb_load_outcome != RB_NO_BANK &&
             rb_load_outcome != RB_INVALID) {
    /* a failed discovery answers every cached verdict in the visit */
    return rb_load_outcome;
  }
  if (rb_card_ready() != RB_OK) {
    return RB_NOT_READY;
  }
  if (op == RB_BANK_RECALL) {
    if (rb_load_outcome == RB_INVALID) {
      return RB_INVALID;
    }
    if (rb_loaded == 0u ||
        (rb_slot_mask(rb_resident) & (uint16_t)(1u << slot)) == 0u) {
      return RB_UNSET;
    }
    if (rb_codes_ok(rb_payload_at(rb_resident, slot)) == 0u ||
        rb_unbound_ok(rb_payload_at(rb_resident, slot)) == 0u) {
      return RB_INVALID;
    }
    return RB_OK;
  }
  if (op == RB_BANK_SAVE) {
    return RB_OK;
  }
  return RB_RANGE;
}

/* the typed status consumer the menu arms route refusals to; the display
 * binding in menu/recipe_bank_probe.c overrides this weak definition and
 * records through rb_status_record */
static uint32_t rb_last_status;

void rb_status_record(enum rb_bank_status status)
{
  rb_last_status = (uint32_t)status;
}

uint32_t rb_status_last(void) { return rb_last_status; }

void rb_status_failed(enum rb_bank_status status) __attribute__((weak));
void rb_status_failed(enum rb_bank_status status) { rb_status_record(status); }
