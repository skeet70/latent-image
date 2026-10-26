/* The editable custom-recipe bank's storage kernel (X-M1 1.50, candidate,
 * non-installable). It owns bank state and the record work the menu hook
 * (menu/recipe_bank_hook.c) asks for.
 *
 * Placement: linked with the menu arms into one blob carried at a plain-extent
 * gap (rebuild-update replace-plain). Slot 0 is the stock C record and is
 * never written here: a selection is refused at the C dial position, the snapshot
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
 * A selection admits its target slot before any write, by the stock list's row
 * check (FUN_00d52684) in the current mode: the property's conversion table
 * carries the code, FUN_00631b74 admits the row's key in the mode's column of
 * the availability matrix when the matrix lists the property, and a property a
 * menu list offers stores a row that list offers. One refused field refuses
 * the selection with nothing written.
 *
 * A bank payload is then applied through the stock apply and recompute path: a
 * stored code that the property's conversion table carries resolves to its row
 * through the same table walk the menu's current-row resolver uses, and the
 * stock commit dispatcher 0x00d4fc74 then runs that property's own arm, its
 * processing and menu consumers (the film cell at 0x009568b8, the tone and
 * pipeline notifications 0x0063c51c/0x00641438) and the shared change notify.
 * A field whose code no row names, the halfwords included, is written through
 * the stock write entry 0x0061c91c, the entry every commit arm uses for its
 * own field. IMAGE SIZE is one commit of the offered row whose 0x007a00cc
 * entry is the stored (0xf6, 0xf9) pair, and IMAGE QUALITY one commit of its
 * row, whose arm writes 0x1d; 0xf9 and 0x1d are never applied on their own.
 * While the quality an apply leaves is RAW-only (0xfa row 4) the IMAGE SIZE
 * pair is skipped, as the stock list greys it, so the size and aspect cells
 * keep the values the 0xfa arm set. A second pass re-applies any field
 * another arm rewrote (bracketing's arm writes 0x1b1), and one assert pass
 * then reads every field back through the stock current-value reader. A
 * field that does not hold its stored code answers RB_APPLY_REFUSED after the
 * live codes read before the apply pass are applied again through the same
 * passes and read back; under a live RAW-only quality that restore writes
 * the IMAGE SIZE pair through the write entry instead of its commit.
 *
 * ISO AUTO (0x141 code 0x1b) is the state the stock ISO list leaves: the
 * mode's auto flag (table 0x007a0018) at 0 and the mode's ISO property (table
 * 0x007a13c8: 0x141 in modes 0x21-0x25, 0x157 in mode 0) at the code of the
 * step FUN_00643e58 takes from the mode's AUTO maximum (table 0x007a09fc),
 * never 0x1b. A payload whose flag field for the mode reads 0, as Save
 * Current writes in AUTO, commits row 0x1b. The commit of row 0x1b writes
 * both; it runs between the apply pass and the re-apply pass, both of which
 * skip 0x141 and the flag, and the payload's AUTO maximum is applied right
 * before it. In mode cells 0x1f-0x21 the ISO, AUTO-maximum and DR arms can
 * reset 0x1c6 to 0x7f (FUN_0063d5fc). A recall whose DR or ISO commit leaves
 * it reset skips 0x1c6; otherwise the re-apply pass writes the slot's 0x1c6
 * back after the AUTO-maximum commit's reset.
 *
 * Open and write producer shims copy the accepted identity into their queued
 * payloads. The destructive filesystem calls are guarded in the media worker,
 * which also executes the attach and detach handlers. Stale requests complete
 * without opening, writing or flushing a replacement filesystem context.
 * Post-completion checks prevent publishing across a media transition.
 * Close ownership is released in the media task through the stock tag helpers.
 */
#include <stdint.h>
#include <stdbool.h>

#include "recipe_bank_ui.h"
#ifdef RB_RAM_DIAGNOSTICS
#include "recipe_bank_diagnostic.h"
#define RB_DISCOVERY_IMPL(name) name##_observed
#define RB_DISCOVERY_PARAMETER , RbDiscoveryRequest discovery
#define RB_DISCOVERY_ARGUMENT , discovery
#define RB_DISCOVERY_UNOBSERVED , (RbDiscoveryRequest){{0u}, {0u}, RB_DISCOVERY_NO_FILE}
#else
#define RB_DISCOVERY_IMPL(name) name
#define RB_DISCOVERY_PARAMETER
#define RB_DISCOVERY_ARGUMENT
#define RB_DISCOVERY_UNOBSERVED
#endif

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
#define RB_MODE_P 0x21u           /* recall runs in mode cells P, S, A and M only */
#define RB_MODE_M 0x24u
#define RB_AVAILABLE 0x00631b74u  /* (mode, prop, key) -> 1 when the cell is 1 or 2 */
#define RB_AVAIL_COUNT 0x00631edcu /* (mode, prop) -> available keys in the mode's column, capped at 2 */
#define RB_PROP_AUTO_MAX_0 0x14fu /* the matrix FUN_00d52684 counts for the AUTO row */
#define RB_PROP_1C6 0x1c6u        /* table-less; stored as key + 0x7f (0x006464b0/0x006464cc) */
#define RB_1C6_BIAS 0x7fu
#define RB_1C6_KEY_SET 0x00633eacu /* (key): stores the key byte in 0x009568d0 */
#define RB_1C6_COMMIT 0x006464ccu  /* writes 0x1c6 = 0x009568d0 + 0x7f through 0x0061c91c */
#define RB_PROP_RATE 0x214u       /* drive row 1's rate; rows 3 and 7 hold 0x216 and 0x215 */
#define RB_PROP_RATE_2 0x217u     /* drive row 1's second rate; rows 3 and 7 hold 0x218 and 0x219 */
#define RB_DRIVE_ROW 0x00956844u  /* drive row of 0x1b1, set by FUN_0063386c */
#define RB_AVAIL_LIST 0x0079944cu /* {prop, form, rows} per listed property, rows 0 ends */
#define RB_SHOOTING_MENU 0x0078ea98u
#define RB_WB_MENU 0x0077f868u    /* WHITE BALANCE's list, which no shooting-menu row opens */
#define RB_SIZE_PAIRS 0x007a00ccu /* 16 bytes per IMAGE SIZE row: 0xf6 code, 0xf9 code */
#define RB_VALUES_END 0x7ffffffeu
#define RB_ROW_NONE 0xffffffffu
#define RB_PROP_SIZE 0xf6u
#define RB_PROP_ASPECT 0xf9u
#define RB_PROP_QUALITY 0xfau
#define RB_PROP_RAW 0x1du
#define RB_PROP_WB 0xedu
#define RB_MODE_CELL 0x00956838u      /* the shooting mode index */
#define RB_AUTO_FLAGS 0x007a0018u     /* + 4*mode -> auto-ISO flag property, 0 none */
#define RB_AUTO_MAXIMA 0x007a09fcu    /* + 4*mode -> AUTO-maximum property, 0 none */
#define RB_ISO_TARGETS 0x007a13c8u    /* + 4*mode -> property FUN_00644848 writes, 0 none */
#define RB_AUTO_FLAG_READ 0x00643d94u /* (mode) -> auto flag, 1 without one */
#define RB_ISO_STEP 0x009568b4u       /* the step FUN_00644848 converts into 0x141 */
#define RB_ISO_PROP 0x141u
#define RB_ISO_AUTO 0x1bu

/* v2 bank file: the 32-byte typed header, the selection block at 0x20..0x3f,
 * the base payload at 0x40, seven records of name, saved and working payload,
 * zero to 1024. v1 (512 bytes: header, seven payloads, seven names) is read
 * only for migration. */
#define RB_PAYLOAD 0x28u
#define RB_NAME 26u
#define RB_NAME_UNITS 12u
#define RB_FIRMWARE_TAG 150u
#define RB_HEADER 64u
#define RB_SELECTION_AT 0x20u
#define RB_AUTO_AT 0x21u
#define RB_BASE_VALID_AT 0x22u
#define RB_BASE_MODE_AT 0x23u
#define RB_RESERVED_AT 0x24u
#define RB_BASE_AT 0x40u
#define RB_RECORDS_AT (RB_BASE_AT + RB_PAYLOAD)
#define RB_RECORD (RB_NAME + 2u * RB_PAYLOAD)
#define RB_RECORDS_END (RB_RECORDS_AT + RB_BANK_SLOTS * RB_RECORD)
#define RB_BODY (RB_RECORDS_END - RB_HEADER)
#define RB_FILE_BYTES 1024u
#ifdef RB_CC_BANK
#define RB_FORMAT_VERSION 3u
#else
#define RB_FORMAT_VERSION 2u
#endif
#define RB_MODE_CODE_M 4u
#define RB_V1_HEADER 32u
#define RB_V1_BLOB (RB_BANK_SLOTS * (RB_PAYLOAD + RB_NAME))
#define RB_V1_FILE_BYTES 512u
#define RB_V1_VERSION 1u

#define RB_FILE_A 0u
#define RB_FILE_B 1u
#define RB_FILE_NONE 0xffu

/* one scanned file: absent or not a bank file, a checksum-valid bank holding
 * a payload no stock menu produces, or a load candidate */
enum rb_file_class {
  RB_CLASS_NONE = 0,
  RB_CLASS_UNSAFE = 1,
  RB_CLASS_VALID = 2
};

RbMediaIdentity rb_media_sample(void)
{
  RbMediaIdentity identity = {rb_media_epoch()};
  return identity;
}

bool rb_media_matches(RbMediaIdentity identity)
{
  return rb_media_epoch() == identity.epoch;
}

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
static volatile RbMediaIdentity rb_request_identity;
/* An unloaded resident is zero-filled. */
static uint32_t rb_loaded;
static uint32_t rb_current_gen;
static uint32_t rb_load_outcome = RB_NO_BANK;
static uint32_t rb_stage_slot = 0xffffffffu;

static __attribute__((noinline)) void rb_memset(uint8_t *dst, uint8_t v, uint32_t n)
{
  uint32_t i;
  for (i = 0u; i < n; i++) {
    dst[i] = v;
  }
}

static __attribute__((noinline)) void rb_memcpy(uint8_t *dst, const uint8_t *src, uint32_t n)
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

typedef void (*rb_callback)(uint32_t, uint32_t);

struct rb_media_message {
  uint16_t id;
  uint8_t source;
  uint8_t destination;
  uint32_t length;
  void *payload;
};

/* The accepted identity occupies the recipe pathname's unused tail word. */
struct rb_open_request {
  rb_callback callback;
  uint8_t path[44];
  RbMediaIdentity identity;
  uint32_t saved_tag;
  uint32_t mode;
  uint32_t tag;
  uint32_t completion[3];
  uint32_t wait;
};

struct rb_write_request {
  rb_callback callback;
  uint32_t handle;
  uint32_t length;
  const uint8_t *source;
  uint32_t completion[3];
  RbMediaIdentity identity;
};

struct rb_close_request {
  rb_callback callback;
  uint32_t handle;
  uint32_t mode;
  uint8_t path[16];
  RbMediaIdentity identity;
  uint32_t completion[2];
};

_Static_assert(sizeof(struct rb_media_message) == 12u, "media message ABI");
_Static_assert(sizeof(struct rb_open_request) == 80u, "open request ABI");
_Static_assert(sizeof(struct rb_write_request) == 32u, "write request ABI");
_Static_assert(sizeof(struct rb_close_request) == 40u, "close request ABI");

RbMediaIdentity rb_io_request_identity(void)
{
  RbMediaIdentity identity = {rb_request_identity.epoch};
  return identity;
}

bool rb_media_worker_allows(rb_callback callback, RbMediaIdentity identity)
{
  return callback != rb_io_cb || rb_media_matches(identity);
}

/* The producer frames reserve these payload spans through the stock post call. */
uint32_t rb_media_request_post(struct rb_media_message *message)
{
  uint32_t (*post)(struct rb_media_message *) =
      (uint32_t (*)(struct rb_media_message *))0x0071fde4u;
  if (message->id == 0x2c17u && message->length == 80u) {
    struct rb_open_request *request = message->payload;
    if (request->callback == rb_io_cb) {
      request->identity = rb_io_request_identity();
    }
  } else if (message->id == 0x2c19u && message->length == 28u) {
    struct rb_write_request *request = message->payload;
    if (request->callback == rb_io_cb) {
      request->identity = rb_io_request_identity();
      message->length = sizeof(*request);
    }
  } else if (message->id == 0x2c1au && message->length == 40u) {
    struct rb_close_request *request = message->payload;
    if (request->callback == rb_io_cb) {
      request->identity = rb_io_request_identity();
    }
  }
  return post(message);
}

extern uint32_t rb_call_stock_media_open(struct rb_media_message *message);
extern uint32_t rb_call_stock_media_write(struct rb_media_message *message);
extern uint32_t rb_call_stock_media_close(struct rb_media_message *message);

static void rb_media_complete(rb_callback callback, uint32_t *completion,
                               bool has_value)
{
  void (*signal)(void) = (void (*)(void))0x0071c634u;
  callback(2u, 0u);
  if (completion[0] != 0u) {
    if (completion[1] != 0u) {
      *(volatile uint32_t *)(uintptr_t)completion[1] = 2u;
    }
    if (has_value && completion[2] != 0u) {
      *(volatile uint32_t *)(uintptr_t)completion[2] = 0u;
    }
    signal();
  }
}

/* FUN_0072129c serializes opens through FUN_007217f0 and 0x0091028c.
 * Deferred requests retain their payload in the front task's stack copy. */
static bool rb_media_open_is_recipe(const struct rb_media_message *message,
                                     rb_callback callback)
{
  const rb_callback *saved = (const rb_callback *)(uintptr_t)0x0091028cu;
  return callback == rb_io_cb ||
         (message->id == 0x2c17u && message->length == 80u &&
          message->source == 0x65u && message->destination == 0x1cu &&
          callback == (rb_callback)0x007217f0u && *saved == rb_io_cb);
}

uint32_t rb_media_guarded_open(struct rb_media_message *message)
{
  struct rb_open_request *request = message->payload;
  void (*release)(uint32_t) = (void (*)(uint32_t))0x0071eff4u;
  if (rb_media_open_is_recipe(message, request->callback) &&
      !rb_media_matches(request->identity)) {
    release(request->tag);
    request->saved_tag = 0u;
    rb_media_complete(request->callback, request->completion, true);
    return 2u;
  }
  return rb_call_stock_media_open(message);
}

uint32_t rb_media_guarded_write(struct rb_media_message *message)
{
  struct rb_write_request *request = message->payload;
  if (request->callback == rb_io_cb &&
      !rb_media_worker_allows(request->callback, request->identity)) {
    request->length = 0u;
    rb_media_complete(request->callback, request->completion, true);
    return 2u;
  }
  return rb_call_stock_media_write(message);
}

uint32_t rb_media_guarded_close(struct rb_media_message *message)
{
  struct rb_close_request *request = message->payload;
  void (*release)(uint32_t) = (void (*)(uint32_t))0x0071eff4u;
  void (*delete_mapping)(uint32_t) = (void (*)(uint32_t))0x0071eaacu;
  if (request->callback == rb_io_cb &&
      !rb_media_worker_allows(request->callback, request->identity)) {
    release(request->handle);
    delete_mapping(request->handle);
    rb_media_complete(request->callback, request->completion, false);
    return 2u;
  }
  return rb_call_stock_media_close(message);
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

static uint32_t rb_close_quiet(uint32_t handle, RbMediaIdentity identity);

/* a close that leaves its tag allocated released nothing, and the low-level
 * release would race the media worker's shared open-file counter (0x703f94's
 * unprotected update at 0x93a914), so the release is deferred until a
 * re-dispatched close completes inside the media queue. The list is sized to
 * the stock tag pool (16, 0x71ef18), so every deferred handle is tracked. */
struct rb_deferred_close {
  uint32_t handle;
  RbMediaIdentity identity;
};
static struct rb_deferred_close rb_pending_close[16];
static uint32_t rb_pending_count;
static uint32_t rb_close_deferred_flag;

static void rb_defer_close(uint32_t handle, RbMediaIdentity identity)
{
  uint32_t i;
  if (handle == 0u) {
    return;
  }
  for (i = 0u; i < rb_pending_count; i++) {
    if (rb_pending_close[i].handle == handle) {
      return;
    }
  }
  if (rb_pending_count < 16u) {
    rb_pending_close[rb_pending_count] = (struct rb_deferred_close){handle, identity};
    rb_pending_count++;
  }
}

static void rb_flush_pending(void)
{
  uint32_t kept = 0u;
  uint32_t i;
  for (i = 0u; i < rb_pending_count; i++) {
    if (rb_tag_owned(rb_pending_close[i].handle) == 0u) {
      /* a late completion released it after our await gave up */
      continue;
    }
    (void)rb_close_quiet(rb_pending_close[i].handle, rb_pending_close[i].identity);
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
static uint32_t RB_DISCOVERY_IMPL(rb_close_quiet)(uint32_t handle, RbMediaIdentity identity
                                                 RB_DISCOVERY_PARAMETER)
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
  rb_request_identity = identity;
#ifdef RB_RAM_DIAGNOSTICS
  uint32_t producer = close_fn((void *)rb_io_cb, handle, 0u, 0u, 1u, 0u);
  rb_discovery_completion(discovery, RB_DISCOVERY_CLOSE,
      (RbDiscoverySharedCompletion){{producer}, {rb_io_status}, {rb_io_value}, {rb_io_done}});
#else
  (void)close_fn((void *)rb_io_cb, handle, 0u, 0u, 1u, 0u);
#endif
  if (rb_await() != RB_OK) {
    rb_close_deferred_flag = 1u;
    return RB_IO;
  }
  mapped = rb_map_completion(rb_io_status);
  if (rb_tag_owned(handle) != 0u) {
    rb_close_deferred_flag = 1u;
  }
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  return (mapped == RB_OK && rb_io_status != 0u) ? RB_IO : mapped;
}

#ifdef RB_RAM_DIAGNOSTICS
static uint32_t rb_close_quiet(uint32_t handle, RbMediaIdentity identity)
{
  return rb_close_quiet_observed(handle, identity RB_DISCOVERY_UNOBSERVED);
}
#endif

/* A nonzero callback handle is deferred when the completed identity is stale. */
static uint32_t RB_DISCOVERY_IMPL(rb_open)(uint32_t which, uint32_t mode,
                                          RbMediaIdentity identity, uint32_t *handle_out
                                          RB_DISCOVERY_PARAMETER)
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
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  rb_io_begin();
  rb_request_identity = identity;
#ifdef RB_RAM_DIAGNOSTICS
  rb_discovery_path(discovery, rb_path[which]);
#endif
  tag = open_fn((void *)rb_io_cb, rb_path[which], mode, 1u, 1u, 0u);
#ifdef RB_RAM_DIAGNOSTICS
  rb_discovery_completion(discovery, RB_DISCOVERY_OPEN,
      (RbDiscoverySharedCompletion){{tag}, {rb_io_status}, {rb_io_value}, {rb_io_done}});
#endif
  if (rb_await() != RB_OK) {
    /* no callback: a dispatched request may still complete late, and without
     * the callback's handle the request has no closable identity here */
    return RB_IO;
  }
  if (!rb_media_matches(identity)) {
    /* the identity moved between the request and its completion */
    if (rb_io_value != 0u) {
      rb_defer_close(rb_io_value, identity);
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
      rb_defer_close(rb_io_value, identity);
    }
    return RB_IO;
  }
  *handle_out = rb_io_value;
  return RB_OK;
}

#ifdef RB_RAM_DIAGNOSTICS
static uint32_t rb_open(uint32_t which, uint32_t mode, RbMediaIdentity identity,
                        uint32_t *handle_out)
{
  return rb_open_observed(which, mode, identity, handle_out RB_DISCOVERY_UNOBSERVED);
}
#endif

static uint32_t RB_DISCOVERY_IMPL(rb_read_count)(uint32_t handle, uint8_t *buf, uint32_t len,
                                                uint32_t *count_out, RbMediaIdentity identity
                                                RB_DISCOVERY_PARAMETER)
{
  uint32_t (*read_fn)(void *, uint32_t, uint32_t, uint8_t *, uint32_t,
                      uint32_t) =
      (uint32_t (*)(void *, uint32_t, uint32_t, uint8_t *, uint32_t,
                    uint32_t))RB_BASE_READ;
  if (rb_task_ok() == 0u) {
    return RB_TASK_REFUSED;
  }
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  rb_io_begin();
#ifdef RB_RAM_DIAGNOSTICS
  uint32_t producer = read_fn((void *)rb_io_cb, handle, len, buf, 1u, 0u);
  rb_discovery_completion(discovery, RB_DISCOVERY_READ,
      (RbDiscoverySharedCompletion){{producer}, {rb_io_status}, {rb_io_value}, {rb_io_done}});
#else
  (void)read_fn((void *)rb_io_cb, handle, len, buf, 1u, 0u);
#endif
  bool completed = rb_await() == RB_OK;
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  if (!completed) {
    return RB_IO;
  }
  if (rb_io_status != 0u) {
    uint32_t mapped = rb_map_completion(rb_io_status);
    return (mapped == RB_OK) ? RB_IO : mapped;
  }
  *count_out = rb_io_value;
  return RB_OK;
}

static uint32_t rb_write(uint32_t handle, const uint8_t *src, uint32_t len,
                         RbMediaIdentity identity)
{
  uint32_t (*write_fn)(void *, uint32_t, uint32_t, const uint8_t *, uint32_t,
                       uint32_t) =
      (uint32_t (*)(void *, uint32_t, uint32_t, const uint8_t *, uint32_t,
                    uint32_t))RB_BASE_WRITE;
  if (rb_task_ok() == 0u) {
    return RB_TASK_REFUSED;
  }
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  rb_io_begin();
  rb_request_identity = identity;
  (void)write_fn((void *)rb_io_cb, handle, len, src, 1u, 0u);
  bool completed = rb_await() == RB_OK;
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  if (!completed) {
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
static uint32_t rb_checksum_file(const uint8_t *file, uint32_t length)
{
  uint32_t sum = 0u;
  uint32_t i;
  for (i = 0u; i < length; i++) {
    uint8_t b = (i >= 0x18u && i < 0x1cu) ? 0u : file[i];
    sum += (uint32_t)(uint8_t)~b;
  }
  return ~sum;
}

static uint32_t rb_generation(const uint8_t *file) { return rb_u32(file + 0x10); }
static uint16_t rb_slot_mask(const uint8_t *file) { return rb_u16(file + 0x1cu); }
static uint8_t *rb_name_at(uint8_t *file, uint32_t slot)
{
  return file + RB_RECORDS_AT + slot * RB_RECORD;
}
static uint8_t *rb_saved_at(uint8_t *file, uint32_t slot)
{
  return rb_name_at(file, slot) + RB_NAME;
}
static uint8_t *rb_payload_at(uint8_t *file, uint32_t slot)
{
  return rb_saved_at(file, slot) + RB_PAYLOAD;
}
static uint8_t *rb_v1_payload_at(uint8_t *file, uint32_t slot)
{
  return file + RB_V1_HEADER + slot * RB_PAYLOAD;
}
static uint8_t *rb_v1_name_at(uint8_t *file, uint32_t slot)
{
  return file + RB_V1_HEADER + RB_BANK_SLOTS * RB_PAYLOAD + slot * RB_NAME;
}

static uint32_t rb_zero(const uint8_t *p, uint32_t n)
{
  uint32_t i;
  for (i = 0u; i < n; i++) {
    if (p[i] != 0u) {
      return 0u;
    }
  }
  return 1u;
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

/* the conversion walk with its not-found flag: `found` is 1 only when a row
 * matched and the wanted column is not the 0x7ffffffe sentinel */
static uint32_t rb_lookup(uint32_t prop, uint32_t value, uint32_t column,
                          uint32_t want, uint32_t *found)
{
  volatile uint32_t *miss = (volatile uint32_t *)(uintptr_t)RB_TABLE_MISS;
  uint32_t result;
  *miss = 0u;
  result = rb_convert(prop, value, column, want);
  *found = (*miss == 0u) ? 1u : 0u;
  return result;
}

/* the row a stored code commits through, and whether a row carried it: the
 * same direction the menu's current-row resolver 0x00d588b8 uses */
static uint32_t rb_code_row(uint32_t prop, uint32_t code, uint32_t *found)
{
  return rb_lookup(prop, code, 1u, 0u, found);
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
 * notify; 0x1c6 goes through the stock pair that sets its key cell
 * 0x009568d0 and writes the property from it; every other code, the
 * halfwords included, goes through the stock write entry the shared arm uses
 * for its field. The 0x214 and 0x217 arms write the property of the current
 * drive row (FUN_00646af0, FUN_00646c04), which is 0x214 and 0x217 only at
 * row 1, so at any other row both go through the write entry. */
static void rb_apply_prop(uint32_t prop, uint32_t code)
{
  uint32_t (*write)(uint32_t, uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t, uint32_t))RB_WRITE;
  uint32_t (*commit)(uint32_t, uint32_t, uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t, uint32_t, uint32_t))RB_COMMIT;
  uint32_t found = 0u;
  uint32_t row;
  if ((prop == RB_PROP_RATE || prop == RB_PROP_RATE_2) &&
      *(volatile uint32_t *)(uintptr_t)RB_DRIVE_ROW != 1u) {
    (void)write(prop, code, 0u);
    return;
  }
  row = rb_code_row(prop, code, &found);
  if (found != 0u) {
    (void)commit(prop, row, 0u, 0u);
    return;
  }
  if (prop == RB_PROP_1C6) {
    ((void (*)(uint32_t))RB_1C6_KEY_SET)(code - RB_1C6_BIAS);
    ((void (*)(void))RB_1C6_COMMIT)();
    return;
  }
  (void)write(prop, code, 0u);
}

/* the current value of a property through the stock reader, which resolves the
 * slot through 0x006439e0: the ordinary block away from the C position */
static uint32_t rb_current(uint32_t prop)
{
  uint32_t (*current)(uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t))RB_CURRENT;
  return current(prop, 0u);
}

static const struct rb_field *rb_walked_field(uint32_t prop)
{
  uint32_t f;
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    if (RB_FIELDS[f].prop == prop) {
      return &RB_FIELDS[f];
    }
  }
  return 0;
}

/* the mode's entry of a per-mode property table (RB_AUTO_FLAGS, RB_AUTO_MAXIMA,
 * RB_ISO_TARGETS), 0 when the mode has none */
static uint32_t rb_mode_prop(uint32_t table, uint32_t mode)
{
  return *(const uint32_t *)(uintptr_t)(table + 4u * mode);
}

enum rb_film_encoding {
  RB_FILM_CURRENT,
  RB_FILM_D3_V2
};

static uint32_t rb_codes_ok_for(const uint8_t *payload,
                                enum rb_film_encoding encoding)
{
  uint32_t f;
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    uint32_t code = rb_extract(payload, &RB_FIELDS[f]);
    if (encoding == RB_FILM_D3_V2 && RB_FIELDS[f].prop == 0xfeu) {
      if (code == 15u) {
        return 0u;
      }
      if (code == 13u) {
        code = 15u;
      }
    }
    if (rb_code_ok(RB_FIELDS[f].prop, code) == 0u) {
      return 0u;
    }
  }
  return 1u;
}

static uint32_t rb_codes_ok(const uint8_t *payload)
{
  return rb_codes_ok_for(payload, RB_FILM_CURRENT);
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

static uint32_t rb_size_row(uint32_t size, uint32_t aspect, uint32_t offered);
static uint32_t rb_quality_raw_code(uint32_t quality, uint32_t *found);

/* a payload a stock menu produces: every table-carried code is a row of its
 * table, (0xf6, 0xf9) is an IMAGE SIZE row the IMAGE SIZE list offers, and
 * 0x1d is the RAW code the 0xfa row's commit arm writes */
static uint32_t rb_payload_safe_for(const uint8_t *payload,
                                    enum rb_film_encoding encoding)
{
  uint32_t found;
  uint32_t raw;
  if (rb_codes_ok_for(payload, encoding) == 0u ||
      rb_size_row(rb_extract(payload, rb_walked_field(RB_PROP_SIZE)),
                  rb_extract(payload, rb_walked_field(RB_PROP_ASPECT)),
                  1u) == RB_ROW_NONE) {
    return 0u;
  }
  raw = rb_quality_raw_code(rb_extract(payload, rb_walked_field(RB_PROP_QUALITY)),
                            &found);
  return (found != 0u && raw == rb_extract(payload, rb_walked_field(RB_PROP_RAW)))
             ? 1u
             : 0u;
}

static uint32_t rb_payload_safe(const uint8_t *payload)
{
  return rb_payload_safe_for(payload, RB_FILM_CURRENT);
}

static uint32_t rb_header_ok(const uint8_t *file, uint16_t version,
                             uint16_t header, uint32_t body)
{
#ifdef RB_CC_BANK
  if (version != RB_FORMAT_VERSION && version != 2u) {
    return 0u;
  }
#endif
  return (file[0] == 'F' && file[1] == 'J' && file[2] == 'R' && file[3] == 'B' &&
          rb_u16(file + 0x04u) == version && rb_u16(file + 0x06u) == header &&
          rb_u16(file + 0x08u) == RB_FIRMWARE_TAG &&
          rb_u16(file + 0x0au) == RB_BANK_SLOTS &&
          rb_u16(file + 0x0cu) == RB_NAME_UNITS &&
          rb_u16(file + 0x0eu) == RB_PAYLOAD && rb_u32(file + 0x14u) == body &&
          rb_u16(file + 0x1eu) == 0u &&
          (uint16_t)(rb_slot_mask(file) & ~(uint16_t)((1u << RB_BANK_SLOTS) - 1u)) ==
              0u)
             ? 1u
             : 0u;
}

static uint32_t rb_v1_class(uint8_t *file)
{
  uint32_t unsafe = 0u;
  uint32_t slot;
  if (rb_header_ok(file, RB_V1_VERSION, RB_V1_HEADER, RB_V1_BLOB) == 0u ||
      rb_checksum_file(file, RB_V1_FILE_BYTES) != rb_u32(file + 0x18u) ||
      rb_zero(file + RB_V1_HEADER + RB_V1_BLOB,
              RB_V1_FILE_BYTES - RB_V1_HEADER - RB_V1_BLOB) == 0u) {
    return RB_CLASS_NONE;
  }
  for (slot = 0u; slot < RB_BANK_SLOTS; slot++) {
    const uint8_t *payload = rb_v1_payload_at(file, slot);
    if (rb_name_ok(rb_v1_name_at(file, slot)) == 0u || rb_unbound_ok(payload) == 0u) {
      return RB_CLASS_NONE;
    }
    if ((rb_slot_mask(file) & (uint16_t)(1u << slot)) != 0u &&
        rb_payload_safe(payload) == 0u) {
      unsafe = 1u;
    }
  }
  return (unsafe != 0u) ? RB_CLASS_UNSAFE : RB_CLASS_VALID;
}

/* A selection names a populated slot and requires a valid base; the base mode
 * is 1-4 (P, S, A, M) exactly when the base is valid; an unpopulated record is
 * zero; ENABLE holds working equal to saved. */
static uint32_t rb_v2_class(uint8_t *file)
{
  uint16_t mask = rb_slot_mask(file);
  uint32_t selection = file[RB_SELECTION_AT];
  uint32_t base_valid = file[RB_BASE_VALID_AT];
  uint32_t mode = file[RB_BASE_MODE_AT];
  uint32_t unsafe = 0u;
  uint32_t slot;
  uint16_t version = RB_FORMAT_VERSION;
  enum rb_film_encoding encoding = RB_FILM_CURRENT;
#ifdef RB_CC_BANK
  if (rb_u16(file + 0x04u) == 2u) {
    version = 2u;
    encoding = RB_FILM_D3_V2;
  }
#endif
  if (rb_header_ok(file, version, RB_HEADER, RB_BODY) == 0u ||
      rb_checksum_file(file, RB_FILE_BYTES) != rb_u32(file + 0x18u) ||
      file[RB_AUTO_AT] > 1u || base_valid > 1u ||
      ((base_valid == 0u) ? (mode != 0u) : (mode == 0u || mode > RB_MODE_CODE_M)) ||
      selection > RB_BANK_SLOTS ||
      (selection != 0u &&
       (base_valid == 0u || (mask & (uint16_t)(1u << (selection - 1u))) == 0u)) ||
      rb_zero(file + RB_RESERVED_AT, RB_BASE_AT - RB_RESERVED_AT) == 0u ||
      rb_zero(file + RB_RECORDS_END, RB_FILE_BYTES - RB_RECORDS_END) == 0u) {
    return RB_CLASS_NONE;
  }
  if (base_valid == 0u) {
    if (rb_zero(file + RB_BASE_AT, RB_PAYLOAD) == 0u) {
      return RB_CLASS_NONE;
    }
  } else if (rb_unbound_ok(file + RB_BASE_AT) == 0u) {
    return RB_CLASS_NONE;
  } else if (rb_payload_safe_for(file + RB_BASE_AT, encoding) == 0u) {
    unsafe = 1u;
  }
  for (slot = 0u; slot < RB_BANK_SLOTS; slot++) {
    const uint8_t *saved = rb_saved_at(file, slot);
    const uint8_t *working = rb_payload_at(file, slot);
    if ((mask & (uint16_t)(1u << slot)) == 0u) {
      if (rb_zero(rb_name_at(file, slot), RB_RECORD) == 0u) {
        return RB_CLASS_NONE;
      }
      continue;
    }
    if (rb_name_ok(rb_name_at(file, slot)) == 0u || rb_unbound_ok(saved) == 0u ||
        rb_unbound_ok(working) == 0u ||
        (file[RB_AUTO_AT] != 0u && rb_bytes_equal(saved, working, RB_PAYLOAD) == 0u)) {
      return RB_CLASS_NONE;
    }
    if (rb_payload_safe_for(saved, encoding) == 0u ||
        (rb_bytes_equal(saved, working, RB_PAYLOAD) == 0u &&
         rb_payload_safe_for(working, encoding) == 0u)) {
      unsafe = 1u;
    }
  }
  return (unsafe != 0u) ? RB_CLASS_UNSAFE : RB_CLASS_VALID;
}

/* the generation's header, its reserved bytes and padding zeroed, then the
 * checksum; the selection block, base and records are kept */
static void rb_seal(uint8_t *file, uint32_t generation, uint16_t mask)
{
  rb_memset(file, 0u, RB_SELECTION_AT);
  rb_memset(file + RB_RESERVED_AT, 0u, RB_BASE_AT - RB_RESERVED_AT);
  rb_memset(file + RB_RECORDS_END, 0u, RB_FILE_BYTES - RB_RECORDS_END);
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
  rb_put_u32(file + 0x14u, (uint32_t)RB_BODY);
  rb_put_u16(file + 0x1cu, mask);
  rb_put_u32(file + 0x18u, rb_checksum_file(file, RB_FILE_BYTES));
}

/* the resident state of a v1 bank: populated names and payloads with working
 * equal to saved, NOTHING SELECTED, DISABLE, base invalid */
static void rb_migrate_v1(uint8_t *dst, uint8_t *v1)
{
  uint16_t mask = rb_slot_mask(v1);
  uint32_t slot;
  rb_memset(dst, 0u, RB_FILE_BYTES);
  for (slot = 0u; slot < RB_BANK_SLOTS; slot++) {
    if ((mask & (uint16_t)(1u << slot)) != 0u) {
      rb_memcpy(rb_name_at(dst, slot), rb_v1_name_at(v1, slot), RB_NAME);
      rb_memcpy(rb_saved_at(dst, slot), rb_v1_payload_at(v1, slot), RB_PAYLOAD);
      rb_memcpy(rb_payload_at(dst, slot), rb_v1_payload_at(v1, slot), RB_PAYLOAD);
    }
  }
  rb_seal(dst, rb_generation(v1), mask);
}

#ifdef RB_CC_BANK
static void rb_migrate_v2_payload(uint8_t *payload)
{
  const struct rb_field *film = rb_walked_field(0xfeu);
  if (rb_extract(payload, film) == 13u) {
    rb_place(payload, film, 15u);
  }
}

static void rb_migrate_v2_film(uint8_t *file)
{
  uint32_t slot;
  uint16_t mask = rb_slot_mask(file);
  uint32_t generation = rb_generation(file);
  if (file[RB_BASE_VALID_AT] != 0u) {
    rb_migrate_v2_payload(file + RB_BASE_AT);
  }
  for (slot = 0u; slot < RB_BANK_SLOTS; slot++) {
    if ((mask & (uint16_t)(1u << slot)) != 0u) {
      rb_migrate_v2_payload(rb_saved_at(file, slot));
      rb_migrate_v2_payload(rb_payload_at(file, slot));
    }
  }
  rb_seal(file, generation, mask);
}
#endif

/* load precedence over the checksum-valid files: the higher generation, a tie
 * goes to A. An unsafe file chosen here blocks the bank; otherwise the write
 * target is the other file, and A when neither file is a bank. */
static uint32_t rb_pick_current(uint32_t gen_a, uint32_t class_a, uint32_t gen_b,
                                uint32_t class_b)
{
  if (class_a != RB_CLASS_NONE && (class_b == RB_CLASS_NONE || gen_a >= gen_b)) {
    return RB_FILE_A;
  }
  if (class_b != RB_CLASS_NONE) {
    return RB_FILE_B;
  }
  return RB_FILE_NONE;
}

/* load --------------------------------------------------------------- */

/* Reads one file into `out`. The loader's size query (0x71ce84, size word at
 * +52) runs before any read, so an empty or wrong-length generation is
 * excluded without a read at EOF (raw 0x25 would map to completion 8), and
 * the length selects the v1 or v2 codec. A positively missing file or a fully
 * read malformed file is not a candidate; an operational failure is returned
 * so no target selection or publication runs on it. */
static uint32_t RB_DISCOVERY_IMPL(rb_scan_file)(uint32_t which, uint32_t epoch, uint8_t *out,
                                               uint32_t *gen_out, uint32_t *class_out,
                                               uint32_t *seen_out RB_DISCOVERY_PARAMETER)
{
  uint32_t (*stat_fn)(uint32_t, uint8_t *) =
      (uint32_t (*)(uint32_t, uint8_t *))RB_FILE_STAT;
  uint32_t handle = 0u;
  uint32_t status;
  uint32_t close_status;
  uint32_t count = 0u;
  uint32_t size = 0u;
  uint8_t stat[128];
  *class_out = RB_CLASS_NONE;
  status = RB_DISCOVERY_IMPL(rb_open)(which, 0u, (RbMediaIdentity){epoch}, &handle
                                     RB_DISCOVERY_ARGUMENT);
#ifdef RB_RAM_DIAGNOSTICS
  rb_discovery_stage(discovery, RB_DISCOVERY_OPEN_RESULT,
                      (RbObservedWord){status}, (RbObservedWord){handle});
#endif
  if (status != RB_OK) {
    return (status == RB_MISSING) ? RB_OK : status;
  }
  *seen_out = 1u;
  if (rb_media_epoch() != epoch) {
    status = RB_MEDIA_CHANGED;
  } else {
    status = stat_fn(handle, stat);
#ifdef RB_RAM_DIAGNOSTICS
    rb_discovery_stage(discovery, RB_DISCOVERY_STAT_RETURN,
                        (RbObservedWord){status}, (RbObservedWord){0u});
#endif
    if (rb_media_epoch() != epoch) {
      status = RB_MEDIA_CHANGED;
    } else if (status != 0u) {
      status = (rb_map_completion(status) == RB_OK) ? RB_IO : rb_map_completion(status);
    } else {
      size = rb_u32(stat + 52);
#ifdef RB_RAM_DIAGNOSTICS
      rb_discovery_stage(discovery, RB_DISCOVERY_STAT_SIZE,
                          (RbObservedWord){size}, (RbObservedWord){0u});
#endif
      if (size != RB_FILE_BYTES && size != RB_V1_FILE_BYTES) {
        status = RB_INVALID;
      } else {
        status = RB_DISCOVERY_IMPL(rb_read_count)(handle, out, size, &count,
                                                  (RbMediaIdentity){epoch}
                                                  RB_DISCOVERY_ARGUMENT);
#ifdef RB_RAM_DIAGNOSTICS
        rb_discovery_stage(discovery, RB_DISCOVERY_READ_RESULT,
                            (RbObservedWord){status}, (RbObservedWord){count});
#endif
        if (status == RB_OK && count != size) {
          status = RB_INVALID;
        }
      }
    }
  }
#ifdef RB_RAM_DIAGNOSTICS
  rb_discovery_stage(discovery, RB_DISCOVERY_BEFORE_CLOSE,
                      (RbObservedWord){status}, (RbObservedWord){0u});
  if (status == RB_OK) {
    rb_discovery_snapshot(discovery, out, (RbObservedWord){size}, RB_DISCOVERY_READ_BYTES);
  }
#endif
  close_status = RB_DISCOVERY_IMPL(rb_close_quiet)(handle, (RbMediaIdentity){epoch}
                                                  RB_DISCOVERY_ARGUMENT);
#ifdef RB_RAM_DIAGNOSTICS
  rb_discovery_stage(discovery, RB_DISCOVERY_CLOSE_RESULT,
                      (RbObservedWord){close_status}, (RbObservedWord){0u});
#endif
  if (rb_close_deferred_flag != 0u) {
    rb_defer_close(handle, (RbMediaIdentity){epoch});
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
  *class_out = (size == RB_V1_FILE_BYTES) ? rb_v1_class(out) : rb_v2_class(out);
  *gen_out = rb_generation(out);
#ifdef RB_RAM_DIAGNOSTICS
  rb_discovery_snapshot(discovery, out, (RbObservedWord){size}, RB_DISCOVERY_AFTER_CLASS);
  rb_discovery_stage(discovery, RB_DISCOVERY_CLASS_RESULT,
                      (RbObservedWord){*class_out}, (RbObservedWord){*gen_out});
#endif
  return RB_OK;
}

#ifdef RB_RAM_DIAGNOSTICS
static uint32_t rb_scan_discovery(uint32_t which, uint32_t epoch, uint8_t *out,
                                 uint32_t *gen_out, uint32_t *class_out,
                                 uint32_t *seen_out, RbDiscoveryRequest discovery)
{
  discovery = rb_discovery_scan_begin(discovery, (RbDiscoveryFile)which,
                                      (RbMediaIdentity){epoch}, out);
  uint32_t status = rb_scan_file_observed(which, epoch, out, gen_out, class_out, seen_out,
                                         discovery);
  rb_discovery_stage(discovery, RB_DISCOVERY_SCAN_RESULT,
                      (RbObservedWord){status}, (RbObservedWord){0u});
  return status;
}

static uint32_t rb_scan_file(uint32_t which, uint32_t epoch, uint8_t *out,
                             uint32_t *gen_out, uint32_t *class_out, uint32_t *seen_out)
{
  return rb_scan_file_observed(which, epoch, out, gen_out, class_out, seen_out
                               RB_DISCOVERY_UNOBSERVED);
}
#endif

/* Discovery only: nothing resident is published until both scans complete and
 * the media epoch witness is unchanged across them, and each scan's open is
 * posted only while the identity holds. The winner is computed from this scan
 * alone and a v1 winner is published migrated. RB_NO_BANK (no file at all)
 * and RB_INVALID (bytes seen, no bank file) both publish the empty bank and
 * are benign for save and reset; RB_UNSAFE_BANK publishes nothing.
 * The RAM build skips discovery and validates rb_resident instead. */
static uint32_t RB_DISCOVERY_IMPL(rb_load_bound)(RbMediaIdentity identity
                                                RB_DISCOVERY_PARAMETER)
{
  uint32_t gen_a = 0u;
  uint32_t gen_b = 0u;
  uint32_t class_a = RB_CLASS_NONE;
  uint32_t class_b = RB_CLASS_NONE;
  uint32_t seen = 0u;
  uint32_t epoch = identity.epoch;
  uint32_t status;
  uint32_t current;
  uint8_t *winner;
#ifdef RB_RAM_DIAGNOSTICS
  rb_discovery_load_begin(discovery, identity);
#endif

#if defined(RB_VOLATILE_BANK) && RB_VOLATILE_BANK
  if (rb_task_ok() == 0u) {
    rb_load_outcome = RB_TASK_REFUSED;
    return RB_TASK_REFUSED;
  }
  if (!rb_media_matches(identity)) {
    rb_load_outcome = RB_MEDIA_CHANGED;
    return RB_MEDIA_CHANGED;
  }
  if (rb_loaded == 0u) {
    rb_current_gen = 0u;
    rb_load_outcome = RB_NO_BANK;
    return RB_NO_BANK;
  }
  enum rb_file_class resident_class = (enum rb_file_class)rb_v2_class(rb_resident);
  if (resident_class == RB_CLASS_UNSAFE) {
    rb_load_outcome = RB_UNSAFE_BANK;
    return RB_UNSAFE_BANK;
  }
  if (resident_class != RB_CLASS_VALID) {
    rb_loaded = 0u;
    rb_current_gen = 0u;
    rb_memset(rb_resident, 0u, RB_FILE_BYTES);
    rb_load_outcome = RB_INVALID;
    return RB_INVALID;
  }
#ifdef RB_CC_BANK
  if (rb_u16(rb_resident + 0x04u) == 2u) {
    rb_migrate_v2_film(rb_resident);
  }
#endif
  rb_current_gen = rb_generation(rb_resident);
  rb_load_outcome = RB_OK;
  return RB_OK;
#endif

  if (!rb_media_matches(identity)) {
    rb_load_outcome = RB_MEDIA_CHANGED;
    return RB_MEDIA_CHANGED;
  }
#ifdef RB_RAM_DIAGNOSTICS
  status = rb_scan_discovery(RB_FILE_A, epoch, rb_staging, &gen_a, &class_a, &seen, discovery);
#else
  status = rb_scan_file(RB_FILE_A, epoch, rb_staging, &gen_a, &class_a, &seen);
#endif
  if (status != RB_OK) {
    rb_load_outcome = status;
    return status;
  }
#ifdef RB_RAM_DIAGNOSTICS
  status = rb_scan_discovery(RB_FILE_B, epoch, rb_verify, &gen_b, &class_b, &seen, discovery);
#else
  status = rb_scan_file(RB_FILE_B, epoch, rb_verify, &gen_b, &class_b, &seen);
#endif
  if (status != RB_OK) {
    rb_load_outcome = status;
    return status;
  }
  if (rb_media_epoch() != epoch) {
    rb_load_outcome = RB_MEDIA_CHANGED;
    return RB_MEDIA_CHANGED;
  }
  current = rb_pick_current(gen_a, class_a, gen_b, class_b);
#ifdef RB_RAM_DIAGNOSTICS
  rb_discovery_winner(discovery, (RbDiscoveryFile)current);
#endif
  if (current == RB_FILE_NONE) {
    rb_loaded = 0u;
    rb_current_gen = 0u;
    rb_memset(rb_resident, 0u, RB_FILE_BYTES);
    rb_load_outcome = (seen != 0u) ? RB_INVALID : RB_NO_BANK;
    return rb_load_outcome;
  }
  if (((current == RB_FILE_A) ? class_a : class_b) == RB_CLASS_UNSAFE) {
    rb_load_outcome = RB_UNSAFE_BANK;
    return RB_UNSAFE_BANK;
  }
  winner = (current == RB_FILE_A) ? rb_staging : rb_verify;
  if (rb_u16(winner + 0x04u) == RB_V1_VERSION) {
    rb_migrate_v1(rb_resident, winner);
  } else {
    rb_memcpy(rb_resident, winner, RB_FILE_BYTES);
  }
#ifdef RB_CC_BANK
  if (rb_u16(winner + 0x04u) == 2u) {
    rb_migrate_v2_film(rb_resident);
  }
#endif
  rb_current_gen = rb_generation(winner);
  rb_loaded = 1u;
  rb_load_outcome = RB_OK;
  return RB_OK;
}

#ifdef RB_RAM_DIAGNOSTICS
static uint32_t rb_load_bound(RbMediaIdentity identity)
{
  return rb_load_bound_observed(identity RB_DISCOVERY_UNOBSERVED);
}
#endif

uint32_t rb_load(void)
{
  return rb_load_bound(rb_media_sample());
}

/* The staged generation is validated whole before publication. The SD build
 * writes it to the nonwinning file and reads it back before publishing. */
static uint32_t rb_write_back(uint32_t generation, uint16_t mask,
                              RbMediaIdentity identity)
{
  uint32_t gen_a = 0u;
  uint32_t gen_b = 0u;
  uint32_t class_a = RB_CLASS_NONE;
  uint32_t class_b = RB_CLASS_NONE;
  uint32_t seen = 0u;
  uint32_t current;
  uint32_t target;
  uint32_t handle = 0u;
  uint32_t close_status;
  uint32_t status;

  if (generation == 0u) {
    return RB_GEN_EXHAUST;
  }
  rb_seal(rb_staging, generation, mask);
  if (rb_v2_class(rb_staging) != RB_CLASS_VALID) {
    return RB_INVALID;
  }
#if defined(RB_VOLATILE_BANK) && RB_VOLATILE_BANK
  if (rb_task_ok() == 0u) {
    return RB_TASK_REFUSED;
  }
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  if (rb_loaded != 0u && rb_v2_class(rb_resident) == RB_CLASS_UNSAFE) {
    return RB_UNSAFE_BANK;
  }
  rb_memcpy(rb_resident, rb_staging, RB_FILE_BYTES);
  rb_loaded = 1u;
  rb_current_gen = generation;
  rb_load_outcome = RB_OK;
  return RB_OK;
#endif

  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  status = rb_scan_file(RB_FILE_A, identity.epoch, rb_verify, &gen_a, &class_a, &seen);
  if (status != RB_OK) {
    return status;
  }
  status = rb_scan_file(RB_FILE_B, identity.epoch, rb_verify, &gen_b, &class_b, &seen);
  if (status != RB_OK) {
    return status;
  }
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  current = rb_pick_current(gen_a, class_a, gen_b, class_b);
  if (current != RB_FILE_NONE &&
      ((current == RB_FILE_A) ? class_a : class_b) == RB_CLASS_UNSAFE) {
    return RB_UNSAFE_BANK;
  }
  target = (current == RB_FILE_A) ? RB_FILE_B : RB_FILE_A;

  status = rb_open(target, 1u, identity, &handle);
  if (status != RB_OK) {
    return status;
  }
  status = rb_write(handle, rb_staging, RB_FILE_BYTES, identity);
  close_status = rb_close_quiet(handle, identity);
  if (rb_close_deferred_flag != 0u) {
    rb_defer_close(handle, identity);
  }
  if (close_status != RB_OK && status == RB_OK) {
    status = RB_FULL;
  }
  if (status != RB_OK) {
    return status;
  }
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }

  status = rb_scan_file(target, identity.epoch, rb_verify, &gen_a, &class_a, &seen);
  if (status != RB_OK) {
    return status;
  }
  if (class_a != RB_CLASS_VALID || gen_a != generation ||
      rb_bytes_equal(rb_verify, rb_staging, RB_FILE_BYTES) == 0u) {
    return RB_FULL;
  }
  if (!rb_media_matches(identity)) {
    return RB_MEDIA_CHANGED;
  }
  rb_memcpy(rb_resident, rb_staging, RB_FILE_BYTES);
  rb_loaded = 1u;
  rb_current_gen = generation;
  return RB_OK;
}

/* record work ------------------------------------------------------- */

static uint8_t rb_stage_name[RB_NAME];
static RbMediaIdentity rb_stage_identity;

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

/* a payload of the live settings, the slot Save Current writes and the seed
 * of an unset slot */
static void rb_payload_from_live(uint8_t *payload)
{
  uint32_t (*live)(void) = (uint32_t (*)(void))RB_LIVE_SLOT;
  rb_memset(payload, 0u, RB_PAYLOAD);
  rb_walk_payload_from(live(), payload);
  rb_payload_normalize(payload);
}

static void rb_stage_image(void)
{
  rb_memcpy(rb_staging, rb_resident, RB_FILE_BYTES);
}

uint32_t rb_readback(uint32_t slot, uint8_t *name_out, uint8_t *payload_out,
                     RbMediaIdentity identity)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  status = rb_load_bound(identity);
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  rb_memcpy(name_out, rb_name_at(rb_resident, slot), RB_NAME);
  rb_memcpy(payload_out, rb_payload_at(rb_resident, slot), RB_PAYLOAD);
  return RB_OK;
}

/* saved and working both take the live payload */
uint32_t rb_save_current(uint32_t slot, RbMediaIdentity identity)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  status = rb_load_bound(identity);
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  rb_stage_image();
  rb_payload_from_live(rb_saved_at(rb_staging, slot));
  rb_memcpy(rb_payload_at(rb_staging, slot), rb_saved_at(rb_staging, slot), RB_PAYLOAD);
  return rb_write_back(rb_current_gen + 1u,
                       (uint16_t)(rb_slot_mask(rb_resident) |
                                  (uint16_t)(1u << slot)), identity);
}

/* clears the record of a slot that is not the persisted selection */
uint32_t rb_reset(uint32_t slot, RbMediaIdentity identity)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  status = rb_load_bound(identity);
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  if (rb_resident[RB_SELECTION_AT] == slot + 1u) {
    return RB_SLOT_SELECTED;
  }
  rb_stage_image();
  rb_memset(rb_name_at(rb_staging, slot), 0u, RB_RECORD);
  return rb_write_back(rb_current_gen + 1u,
                       (uint16_t)(rb_slot_mask(rb_resident) &
                                  (uint16_t) ~(1u << slot)), identity);
}

/* The last selection that did not return RB_OK or skipped a field, for a host
 * RAM read after the session. `selection` is the requested selection code.
 * `prop` is the first refused field: of the admission pass, which refuses a
 * slot holding a code no row of its table carries or an IMAGE SIZE or RAW code
 * no offered row produces, before any write; or else of the assert pass. It is
 * 0 for the other refusals before the apply pass, a mode cell outside P, S, A
 * and M among them.
 * `refused_fields` counts every refused field and `restore_refused` the fields
 * that did not read back their pre-transaction code after a refusal re-applied
 * it. `skipped_fields` counts the target's fields the selection does not
 * write, which keep their base codes unless another field's commit changes
 * them (0x1c6 under a reset), and `skipped_prop` is the first of them. */
typedef struct {
  uint32_t magic;
  uint32_t count;
  uint32_t status;
  uint32_t selection;
  uint32_t prop;
  uint32_t stored;
  uint32_t read_back;
  uint32_t mode;
  uint32_t refused_fields;
  uint32_t restore_refused;
  uint32_t skipped_fields;
  uint32_t skipped_prop;
} RbSelectTrace;

#define RB_SELECT_TRACE_MAGIC 0x52424352u /* "RCBR" */

__attribute__((used)) volatile RbSelectTrace rb_select_trace = {
  RB_SELECT_TRACE_MAGIC, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u, 0u
};

static void rb_refuse(RbSelectTrace *trace, uint32_t prop, uint32_t stored,
                      uint32_t read_back)
{
  if (trace->refused_fields == 0u) {
    trace->prop = prop;
    trace->stored = stored;
    trace->read_back = read_back;
  }
  trace->refused_fields++;
}

/* The properties whose commit writes the mode's own property: the arm stores
 * the code into `targets`[mode] (0 when the mode has none) and never into the
 * source itself outside the modes whose entry is the source. The walk carries
 * the targets of some modes as separate fields. RB_FAMILY_MAXIMUM is the ISO
 * AUTO maximum, applied only with the ISO commit, whose 0x141 arm also writes
 * the mode's flag (table 0x007a0018, FUN_00643dc4) and target (table
 * 0x007a13c8, FUN_00644848). */
struct rb_family {
  uint32_t source;
  uint32_t targets;
};

static const struct rb_family RB_FAMILIES[] = {
  {0x104u, 0x007a1120u}, /* DR, FUN_00643fb8 */
  {0x14bu, 0x007a0eb8u}, /* FUN_00643e30 */
  {0x153u, 0x007a1314u}, /* FUN_00643f08 */
  {0x14fu, RB_AUTO_MAXIMA}, /* FUN_00643e9c */
};

#define RB_FAMILY_COUNT (sizeof RB_FAMILIES / sizeof RB_FAMILIES[0])
#define RB_FAMILY_DR 0u
#define RB_FAMILY_MAXIMUM 3u

/* X-M1 1.50's six family tables have 0x2d words (mode 0 through 0x2c):
 * 0x007a0018 ends at 0x007a00cc; 0x007a1314 at 0x007a13c8;
 * 0x007a13c8 at 0x007a147c. These are their nonzero property union. */
static uint32_t rb_family_member(uint32_t prop)
{
  return (prop == 0x104u ||
          (prop >= 0x109u && prop <= 0x10bu) ||
          prop == RB_ISO_PROP || prop == 0x142u ||
          (prop >= 0x144u && prop <= 0x146u) ||
          (prop >= 0x14bu && prop <= 0x157u) ||
          (prop >= 0x15bu && prop <= 0x160u))
             ? 1u
             : 0u;
}

static uint32_t rb_word(uint32_t addr)
{
  return *(const volatile uint32_t *)(uintptr_t)addr;
}

/* FUN_00631b74's descriptor search: whether the availability matrix lists the
 * property */
static uint32_t rb_avail_listed(uint32_t prop)
{
  uint32_t entry;
  for (entry = RB_AVAIL_LIST; rb_word(entry + 8u) != 0u; entry += 12u) {
    if (rb_word(entry) == prop) {
      return 1u;
    }
  }
  return 0u;
}

/* the values array of the list a property's menu row opens, read from the
 * live node words; 0 for a property no menu list offers */
static uint32_t rb_menu_values(uint32_t prop)
{
  uint32_t row;
  if (prop == RB_PROP_WB) {
    return rb_word(RB_WB_MENU + RB_NODE_VALUES);
  }
  for (row = rb_word(RB_SHOOTING_MENU + RB_NODE_VALUES);
       rb_word(row) != RB_VALUES_END; row += 8u) {
    uint32_t child = rb_word(row + 4u);
    if (child != 0u && rb_word(child + RB_NODE_X) == prop) {
      return rb_word(child + RB_NODE_VALUES);
    }
  }
  return 0u;
}

static uint32_t rb_row_offered(uint32_t values, uint32_t row)
{
  for (; rb_word(values) != RB_VALUES_END; values += 8u) {
    if (rb_word(values) == row) {
      return 1u;
    }
  }
  return 0u;
}

/* the IMAGE SIZE row whose 0x007a00cc entry is (size, aspect), limited to the
 * rows the IMAGE SIZE list offers when `offered` is set */
static uint32_t rb_size_row(uint32_t size, uint32_t aspect, uint32_t offered)
{
  uint32_t values = rb_menu_values(RB_PROP_SIZE);
  uint32_t row;
  uint32_t found;
  for (row = 0u;; row++) {
    (void)rb_lookup(RB_PROP_SIZE, row, 0u, 1u, &found);
    if (found == 0u) {
      return RB_ROW_NONE;
    }
    if (rb_word(RB_SIZE_PAIRS + 16u * row) == size &&
        rb_word(RB_SIZE_PAIRS + 16u * row + 4u) == aspect &&
        (offered == 0u || values == 0u || rb_row_offered(values, row) != 0u)) {
      return row;
    }
  }
}

/* the RAW cell FUN_00633b4c is set to per IMAGE QUALITY row by the 0xfa arm
 * of 0x00d4fc74, which then writes 0x1d as that cell's 0x1d code. Row 4
 * (RAW-only) sets the cell to 1, and the arm then sets the size and aspect
 * cells from 0x00951550/0x00951558. */
static const uint8_t RB_QUALITY_RAW[] = {0u, 0u, 2u, 2u, 1u};
#define RB_QUALITY_RAW_ONLY 4u

static uint32_t rb_quality_raw_code(uint32_t quality, uint32_t *found)
{
  uint32_t row = rb_code_row(RB_PROP_QUALITY, quality, found);
  if (*found == 0u || row >= sizeof RB_QUALITY_RAW) {
    *found = 0u;
    return 0u;
  }
  return rb_lookup(RB_PROP_RAW, RB_QUALITY_RAW[row], 3u, 1u, found);
}

/* one stored code against the stock list's row check in `mode`. ISO AUTO
 * (0x141 row 0x1b) skips the matrix as the stock check does; FUN_00d52684
 * hides it when the 0x14f column has fewer than 2 available keys, and hides
 * every other ISO row when the 0x141 column has. `row` is the row the code
 * commits through. A listed property without a conversion table is checked
 * through its bound key, 0x1c6's stored byte minus 0x7f, and one with no bound
 * key is admitted only at its live code. */
static uint32_t rb_mode_admits(uint32_t prop, uint32_t code, uint32_t row,
                               uint32_t mode)
{
  uint32_t (*available)(uint32_t, uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t, uint32_t))RB_AVAILABLE;
  uint32_t (*count)(uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t))RB_AVAIL_COUNT;
  uint32_t values;
  uint32_t found;
  uint32_t key;
  if (rb_table_ptr(prop) == 0u) {
    if (rb_avail_listed(prop) == 0u) {
      return 1u;
    }
    if (prop == RB_PROP_1C6) {
      return available(mode, prop, code - RB_1C6_BIAS);
    }
    return (rb_current(prop) == code) ? 1u : 0u;
  }
  if (prop == RB_ISO_PROP &&
      count(mode, (code == RB_ISO_AUTO) ? RB_PROP_AUTO_MAX_0 : RB_ISO_PROP) < 2u) {
    return 0u;
  }
  if (!(prop == RB_ISO_PROP && code == RB_ISO_AUTO) && rb_avail_listed(prop) != 0u) {
    key = rb_lookup(prop, code, 1u, 3u, &found);
    if (found == 0u || available(mode, prop, key) == 0u) {
      return 0u;
    }
  }
  values = rb_menu_values(prop);
  return (values == 0u || rb_row_offered(values, row) != 0u) ? 1u : 0u;
}

#define RB_NONE 0xffffffffu
#define RB_SKIP_WORDS ((RB_FIELD_COUNT + 31u) / 32u)

/* the codes one apply run writes: a slot's payload, or the snapshot of the
 * live codes that a refused recall re-applies */
struct rb_apply {
  const uint8_t *payload;
  uint32_t mode;
  uint32_t skip[RB_SKIP_WORDS];     /* RB_FIELDS indices not written */
  uint32_t size_row;                /* the IMAGE SIZE row, RB_ROW_NONE when no row matches */
  uint32_t size_by_write;           /* 1: the IMAGE SIZE pair goes through the write entry */
  uint32_t family[RB_FAMILY_COUNT]; /* the code each family source commits, or RB_NONE */
  uint32_t iso;                     /* the 0x141 code committed, or RB_NONE */
};

/* the walked family members, one bit per RB_FIELDS index */
static uint32_t rb_members[RB_SKIP_WORDS];

static uint32_t rb_bit(const uint32_t *bits, const struct rb_field *f)
{
  uint32_t i = (uint32_t)(f - RB_FIELDS);
  return (bits[i / 32u] >> (i % 32u)) & 1u;
}

static void rb_set_bit(uint32_t *bits, const struct rb_field *f)
{
  uint32_t i = (uint32_t)(f - RB_FIELDS);
  bits[i / 32u] |= 1u << (i % 32u);
}

static void rb_bits_clear(uint32_t *bits)
{
  uint32_t w;
  for (w = 0u; w < RB_SKIP_WORDS; w++) {
    bits[w] = 0u;
  }
}

static void rb_members_init(void)
{
  uint32_t f;
  rb_bits_clear(rb_members);
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    if (rb_family_member(RB_FIELDS[f].prop) != 0u) {
      rb_set_bit(rb_members, &RB_FIELDS[f]);
    }
  }
}

static void rb_skip(struct rb_apply *a, const struct rb_field *f,
                    RbSelectTrace *trace)
{
  if (trace->skipped_fields == 0u) {
    trace->skipped_prop = f->prop;
  }
  trace->skipped_fields++;
  rb_set_bit(a->skip, f);
}

static uint32_t rb_field_admits(const uint8_t *payload, const struct rb_field *f,
                                uint32_t mode)
{
  uint32_t code = rb_extract(payload, f);
  uint32_t found;
  uint32_t row = rb_code_row(f->prop, code, &found);
  return rb_mode_admits(f->prop, code, row, mode);
}

static uint32_t rb_family_target(uint32_t i, uint32_t mode)
{
  return rb_mode_prop(RB_FAMILIES[i].targets, mode);
}

/* whether the mode has an ISO flag, an ISO target and an AUTO maximum */
static uint32_t rb_iso_mode(uint32_t mode)
{
  return (rb_mode_prop(RB_AUTO_FLAGS, mode) != 0u &&
          rb_mode_prop(RB_ISO_TARGETS, mode) != 0u &&
          rb_family_target(RB_FAMILY_MAXIMUM, mode) != 0u)
             ? 1u
             : 0u;
}

/* the 0x141 code a payload commits in `mode`: 0x1b when the payload's auto
 * flag for the mode reads 0. A slot Save Current wrote in AUTO holds the code
 * of the AUTO-maximum step there, with the flag at 0. */
static uint32_t rb_payload_iso(const uint8_t *payload, uint32_t mode)
{
  const struct rb_field *flag = rb_walked_field(rb_mode_prop(RB_AUTO_FLAGS, mode));
  if (flag != 0 && rb_extract(payload, flag) == 0u) {
    return RB_ISO_AUTO;
  }
  return rb_extract(payload, rb_walked_field(RB_ISO_PROP));
}

/* whether the IMAGE QUALITY live after `a` is applied is RAW-only, under
 * which FUN_00d52684(5, 0x3c) greys the IMAGE SIZE row */
static uint32_t rb_raw_only(const struct rb_apply *a)
{
  const struct rb_field *quality = rb_walked_field(RB_PROP_QUALITY);
  uint32_t code = (rb_bit(a->skip, quality) != 0u) ? rb_current(RB_PROP_QUALITY)
                                                    : rb_extract(a->payload, quality);
  uint32_t found;
  uint32_t row = rb_code_row(RB_PROP_QUALITY, code, &found);
  return (found != 0u && row == RB_QUALITY_RAW_ONLY) ? 1u : 0u;
}

/* whether a code's column-3 key, the value the commit arm stores, is found
 * and at most `max` */
static uint32_t rb_key_at_most(uint32_t prop, uint32_t code, uint32_t max)
{
  uint32_t found;
  uint32_t key = rb_lookup(prop, code, 1u, 3u, &found);
  return (found != 0u && key <= max) ? 1u : 0u;
}

/* whether the state a recall leaves has 0x1c6 reset by FUN_0063d5fc, which
 * sets 0x009568d0 to 0 and 0x1c6 to 0x7f. In mode cells 0x1f-0x21 the 0x104
 * arm calls it when DR's key (0x009568bc) is 0, and the 0x141 arm when the
 * ISO step 0x009568b4 it stores is at most 5: the key of a manual ISO code,
 * or for AUTO the 0x14f key of the mode's AUTO maximum. The 0x14f arm's
 * reset, which every AUTO-maximum commit runs, is followed by the ISO commit
 * and stands only when that commit resets too. */
static uint32_t rb_resets_1c6(const struct rb_apply *a)
{
  uint32_t dr = a->family[RB_FAMILY_DR];
  uint32_t maximum = a->family[RB_FAMILY_MAXIMUM];
  if (a->mode < 0x1fu || a->mode > 0x21u) {
    return 0u;
  }
  if (dr != RB_NONE && rb_key_at_most(RB_FAMILIES[RB_FAMILY_DR].source, dr, 0u) != 0u) {
    return 1u;
  }
  if (a->iso == RB_NONE) {
    return 0u;
  }
  if (a->iso != RB_ISO_AUTO) {
    return rb_key_at_most(RB_ISO_PROP, a->iso, 5u);
  }
  if (maximum == RB_NONE) {
    maximum = rb_current(rb_family_target(RB_FAMILY_MAXIMUM, a->mode));
  }
  return rb_key_at_most(RB_PROP_AUTO_MAX_0, maximum, 5u);
}

/* one family's admission: the code is the walked field of the mode's target,
 * or the source's when the walk does not carry that target */
static void rb_admit_family(struct rb_apply *a, uint32_t i, uint32_t allowed,
                            RbSelectTrace *trace)
{
  uint32_t source = RB_FAMILIES[i].source;
  uint32_t target = rb_family_target(i, a->mode);
  const struct rb_field *carrier = rb_walked_field(target);
  uint32_t code;
  uint32_t row;
  uint32_t found;
  if (carrier == 0) {
    carrier = rb_walked_field(source);
  }
  code = rb_extract(a->payload, carrier);
  row = rb_code_row(source, code, &found);
  if (found == 0u) {
    rb_refuse(trace, carrier->prop, code, rb_current(carrier->prop));
  } else if (target == 0u || allowed == 0u ||
             rb_mode_admits(source, code, row, a->mode) == 0u) {
    rb_skip(a, carrier, trace);
  } else {
    a->family[i] = code;
  }
}

/* the admission pass, before any write: returns 0 when a field refuses the
 * recall, and marks the fields the mode does not admit as skipped. The IMAGE
 * SIZE pair and the IMAGE QUALITY/RAW pair are one commit each and are skipped
 * together; the IMAGE SIZE pair is skipped too while the quality the recall
 * leaves is RAW-only. The ISO code is committed only in a mode with an ISO
 * flag, target and maximum, and the AUTO maximum only with it. 0x1c6 is
 * skipped when a family or ISO commit resets it. */
static uint32_t rb_admit(struct rb_apply *a, uint32_t mode, RbSelectTrace *trace)
{
  const struct rb_field *size = rb_walked_field(RB_PROP_SIZE);
  const struct rb_field *aspect = rb_walked_field(RB_PROP_ASPECT);
  const struct rb_field *raw = rb_walked_field(RB_PROP_RAW);
  const struct rb_field *iso = rb_walked_field(RB_ISO_PROP);
  const struct rb_field *key1c6 = rb_walked_field(RB_PROP_1C6);
  uint32_t refused = trace->refused_fields;
  uint32_t f;
  rb_members_init();
  a->mode = mode;
  rb_bits_clear(a->skip);
  a->size_row = RB_ROW_NONE;
  a->size_by_write = 0u;
  a->iso = RB_NONE;
  for (f = 0u; f < RB_FAMILY_COUNT; f++) {
    a->family[f] = RB_NONE;
  }
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    const struct rb_field *field = &RB_FIELDS[f];
    uint32_t code = rb_extract(a->payload, field);
    uint32_t found;
    (void)rb_code_row(field->prop, code, &found);
    if (rb_table_ptr(field->prop) != 0u && found == 0u) {
      rb_refuse(trace, field->prop, code, rb_current(field->prop));
    } else if (rb_bit(rb_members, field) != 0u) {
      continue;
    } else if (field->prop == RB_PROP_SIZE) {
      a->size_row = rb_size_row(code, rb_extract(a->payload, aspect), 1u);
      if (a->size_row == RB_ROW_NONE) {
        rb_refuse(trace, field->prop, code, rb_current(field->prop));
      } else if (rb_mode_admits(field->prop, code, a->size_row, mode) == 0u ||
                 rb_field_admits(a->payload, aspect, mode) == 0u) {
        rb_skip(a, field, trace);
        rb_skip(a, aspect, trace);
      }
    } else if (field->prop == RB_PROP_QUALITY) {
      uint32_t raw_code = rb_quality_raw_code(code, &found);
      if (found == 0u || raw_code != rb_extract(a->payload, raw)) {
        rb_refuse(trace, RB_PROP_RAW, rb_extract(a->payload, raw),
                  rb_current(RB_PROP_RAW));
      } else if (rb_field_admits(a->payload, field, mode) == 0u ||
                 rb_field_admits(a->payload, raw, mode) == 0u) {
        rb_skip(a, field, trace);
        rb_skip(a, raw, trace);
      }
    } else if (field->prop != RB_PROP_ASPECT && field->prop != RB_PROP_RAW &&
               rb_field_admits(a->payload, field, mode) == 0u) {
      rb_skip(a, field, trace);
    }
  }
  if (a->size_row != RB_ROW_NONE && rb_bit(a->skip, size) == 0u && rb_raw_only(a) != 0u) {
    rb_skip(a, size, trace);
    rb_skip(a, aspect, trace);
  }
  for (f = 0u; f < RB_FAMILY_MAXIMUM; f++) {
    rb_admit_family(a, f, 1u, trace);
  }
  if (rb_iso_mode(mode) != 0u) {
    uint32_t code = rb_payload_iso(a->payload, mode);
    uint32_t found;
    uint32_t row = rb_code_row(RB_ISO_PROP, code, &found);
    if (rb_mode_admits(RB_ISO_PROP, code, row, mode) != 0u) {
      a->iso = code;
    }
  }
  if (a->iso == RB_NONE) {
    rb_skip(a, iso, trace);
  }
  rb_admit_family(a, RB_FAMILY_MAXIMUM, (a->iso != RB_NONE) ? 1u : 0u, trace);
  if (rb_bit(a->skip, key1c6) == 0u && rb_resets_1c6(a) != 0u) {
    rb_skip(a, key1c6, trace);
  }
  return (trace->refused_fields == refused) ? 1u : 0u;
}
/* RAW-only restoration writes the stored pair without an IMAGE SIZE commit. */
static uint32_t rb_admit_base(struct rb_apply *base, uint32_t mode, RbSelectTrace *trace)
{
  const struct rb_field *field;
  uint32_t i;
  if (rb_admit(base, mode, trace) == 0u) {
    return 0u;
  }
  if (rb_raw_only(base) != 0u) {
    base->size_by_write = 1u;
    field = rb_walked_field(RB_PROP_SIZE);
    i = (uint32_t)(field - RB_FIELDS);
    base->skip[i / 32u] &= ~(1u << (i % 32u));
    field = rb_walked_field(RB_PROP_ASPECT);
    i = (uint32_t)(field - RB_FIELDS);
    base->skip[i / 32u] &= ~(1u << (i % 32u));
  }
  return 1u;
}


/* the composite partners: the IMAGE SIZE commit writes 0xf9 and the IMAGE
 * QUALITY arm writes 0x1d */
static uint32_t rb_partner_of(uint32_t prop)
{
  if (prop == RB_PROP_SIZE) {
    return RB_PROP_ASPECT;
  }
  return (prop == RB_PROP_QUALITY) ? RB_PROP_RAW : 0u;
}

/* whether the field loop writes a field: RAW and an aspect with an IMAGE SIZE
 * row are written by their partner's commit, the family members by the
 * family commits */
static uint32_t rb_applies(const struct rb_apply *a, const struct rb_field *f)
{
  if (rb_bit(a->skip, f) != 0u || rb_bit(rb_members, f) != 0u ||
      f->prop == RB_PROP_RAW ||
      (f->prop == RB_PROP_ASPECT && a->size_row != RB_ROW_NONE &&
       a->size_by_write == 0u)) {
    return 0u;
  }
  return 1u;
}

static void rb_apply_one(const struct rb_apply *a, const struct rb_field *f)
{
  uint32_t (*write)(uint32_t, uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t, uint32_t))RB_WRITE;
  uint32_t (*commit)(uint32_t, uint32_t, uint32_t, uint32_t) =
      (uint32_t (*)(uint32_t, uint32_t, uint32_t, uint32_t))RB_COMMIT;
  if ((f->prop == RB_PROP_SIZE || f->prop == RB_PROP_ASPECT) && a->size_by_write != 0u) {
    (void)write(f->prop, rb_extract(a->payload, f), 0u);
    return;
  }
  if (f->prop == RB_PROP_SIZE && a->size_row != RB_ROW_NONE) {
    (void)commit(RB_PROP_SIZE, a->size_row, 0u, 0u);
    return;
  }
  rb_apply_prop(f->prop, rb_extract(a->payload, f));
}

/* whether a field, and the partner its commit writes, read back the payload */
static uint32_t rb_holds(const struct rb_apply *a, const struct rb_field *f)
{
  const struct rb_field *partner = rb_walked_field(rb_partner_of(f->prop));
  if (rb_current(f->prop) != rb_extract(a->payload, f)) {
    return 0u;
  }
  return (partner == 0 ||
          rb_current(partner->prop) == rb_extract(a->payload, partner))
             ? 1u
             : 0u;
}

/* another field's arm writes some fields (bracketing's arm writes 0x1b1, the
 * ISO arms' FUN_00641438(2, 0) writes 0x161), so a field that no longer reads
 * back its code is applied once more through the same path */
static void rb_reapply_run(const struct rb_apply *a)
{
  uint32_t f;
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    if (rb_applies(a, &RB_FIELDS[f]) != 0u && rb_holds(a, &RB_FIELDS[f]) == 0u) {
      rb_apply_one(a, &RB_FIELDS[f]);
    }
  }
}

/* the stock apply and recompute pass: every written field goes through its
 * own stock commit, so the processing and menu consumers the menu commits run
 * are run here too. The AUTO maximum is committed before the ISO code because
 * the 0x14f commit also writes the ISO target (FUN_00644848). */
static void rb_apply_run(const struct rb_apply *a)
{
  uint32_t f;
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    if (rb_applies(a, &RB_FIELDS[f]) != 0u) {
      rb_apply_one(a, &RB_FIELDS[f]);
    }
  }
  for (f = 0u; f < RB_FAMILY_COUNT; f++) {
    if (a->family[f] != RB_NONE) {
      rb_apply_prop(RB_FAMILIES[f].source, a->family[f]);
    }
  }
  if (a->iso != RB_NONE) {
    rb_apply_prop(RB_ISO_PROP, a->iso);
  }
  rb_reapply_run(a);
}

static uint32_t rb_assert_one(uint32_t prop, uint32_t code, RbSelectTrace *trace)
{
  uint32_t read_back = rb_current(prop);
  if (read_back == code) {
    return 0u;
  }
  if (trace != 0) {
    rb_refuse(trace, prop, code, read_back);
  }
  return 1u;
}

/* the assert pass, the recall's contract: the number of written properties
 * that do not hold their code, each recorded in `trace` when it is given. A
 * family's code is read from the mode's target. The ISO commit holds when the
 * mode's flag reads 0 for AUTO and 1 otherwise and the mode's ISO target holds
 * the code (for AUTO, the code of the ISO step). */
static uint32_t rb_assert_run(const struct rb_apply *a, RbSelectTrace *trace)
{
  uint32_t refused = 0u;
  uint32_t f;
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    const struct rb_field *field = &RB_FIELDS[f];
    if (rb_bit(a->skip, field) == 0u && rb_bit(rb_members, field) == 0u) {
      refused += rb_assert_one(field->prop, rb_extract(a->payload, field), trace);
    }
  }
  for (f = 0u; f < RB_FAMILY_COUNT; f++) {
    if (a->family[f] != RB_NONE) {
      refused += rb_assert_one(rb_family_target(f, a->mode), a->family[f], trace);
    }
  }
  if (a->iso != RB_NONE) {
    uint32_t auto_iso = (a->iso == RB_ISO_AUTO) ? 1u : 0u;
    refused += rb_assert_one(rb_mode_prop(RB_AUTO_FLAGS, a->mode), 1u - auto_iso, trace);
    refused += rb_assert_one(
        rb_mode_prop(RB_ISO_TARGETS, a->mode),
        auto_iso != 0u ? rb_convert(RB_ISO_PROP,
                                    *(volatile uint32_t *)(uintptr_t)RB_ISO_STEP,
                                    3u, 1u)
                       : a->iso,
        trace);
  }
  return refused;
}

static uint8_t rb_select_snapshot[RB_PAYLOAD];

/* the live code of every walked field and of the mode's family targets
 * through the stock reader. A live ISO AUTO state (the mode's flag reads 0)
 * is recorded as 0x1b, so re-applying it commits row 0x1b as the selection of an
 * AUTO slot does. Under a live RAW-only quality the IMAGE SIZE pair is
 * written through the write entry, so its commit does not replace the size
 * and aspect cells the 0xfa arm sets. */
static void rb_snapshot_live(struct rb_apply *s, uint32_t mode)
{
  uint32_t (*auto_read)(uint32_t) = (uint32_t (*)(uint32_t))RB_AUTO_FLAG_READ;
  uint32_t f;
  rb_memset(rb_select_snapshot, 0u, RB_PAYLOAD);
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    rb_place(rb_select_snapshot, &RB_FIELDS[f], rb_current(RB_FIELDS[f].prop));
  }
  s->payload = rb_select_snapshot;
  s->mode = mode;
  rb_bits_clear(s->skip);
  for (f = 0u; f < RB_FAMILY_COUNT; f++) {
    uint32_t target = rb_family_target(f, mode);
    s->family[f] = (target != 0u) ? rb_current(target) : RB_NONE;
  }
  s->iso = RB_NONE;
  if (rb_iso_mode(mode) != 0u) {
    s->iso = (auto_read(mode) == 0u) ? RB_ISO_AUTO
                                     : rb_current(rb_mode_prop(RB_ISO_TARGETS, mode));
  } else {
    s->family[RB_FAMILY_MAXIMUM] = RB_NONE;
  }
  s->size_row = rb_size_row(rb_extract(rb_select_snapshot, rb_walked_field(RB_PROP_SIZE)),
                            rb_extract(rb_select_snapshot, rb_walked_field(RB_PROP_ASPECT)),
                            0u);
  s->size_by_write = rb_raw_only(s);
}

/* re-applies the live codes read before a transaction and counts the ones
 * that do not read back */
static void rb_restore_run(const struct rb_apply *restore, RbSelectTrace *trace)
{
  rb_apply_run(restore);
  trace->restore_refused = rb_assert_run(restore, 0);
}

/* selection ------------------------------------------------------------ */

/* Base: the ordinary settings are live. Active: the slot's working payload is
 * applied over the persisted base and verified in this session. Suspended: the
 * persisted selection names the slot without a verified apply in this session
 * (a load after boot, or another card). */
typedef enum {
  RB_ACTIVATION_BASE,
  RB_ACTIVATION_ACTIVE,
  RB_ACTIVATION_SUSPENDED
} RbActivationKind;

typedef struct {
  RbActivationKind kind;
  uint32_t slot;
} RbActivation;

/* the v2 base capture-mode byte */
typedef enum {
  RB_ELIGIBLE_P = 1,
  RB_ELIGIBLE_S = 2,
  RB_ELIGIBLE_A = 3,
  RB_ELIGIBLE_M = 4
} RbEligibleMode;

static RbActivation rb_activation = {RB_ACTIVATION_BASE, 0u};
static RbMediaIdentity rb_activation_identity = {0xffffffffu};
static uint8_t rb_select_base[RB_PAYLOAD];
/* The logical footprint checked before the next ordinary-base capture. */
static uint8_t rb_observer_baseline[RB_PAYLOAD];
static uint32_t rb_observer_valid;

__attribute__((noinline)) static void rb_observer_capture(void)
{
  rb_payload_from_live(rb_observer_baseline);
  rb_observer_valid = 1u;
}

/* RESTORE retains the failed selection's original mode and opening media.
 * BASE removes a landed selection's overlay before destination readmission. */
typedef enum {
  RB_PENDING_NONE,
  RB_PENDING_RESTORE,
  RB_PENDING_BASE
} RbPendingKind;

static RbPendingKind rb_pending = RB_PENDING_NONE;
static struct rb_apply rb_pending_restore;
static RbActivation rb_pending_activation;
static RbMediaIdentity rb_pending_identity = {0xffffffffu};
/* a selection or resume is between its first and last live write */
static uint32_t rb_busy;

static uint32_t rb_eligible_mode(uint32_t mode, RbEligibleMode *out)
{
  if (mode < RB_MODE_P || mode > RB_MODE_M) {
    return 0u;
  }
  *out = (RbEligibleMode)(mode - RB_MODE_P + (uint32_t)RB_ELIGIBLE_P);
  return 1u;
}

static uint32_t rb_selection_slot(RbSelection selection, uint32_t *slot_out)
{
  if (selection.code == 0u) {
    return 0u;
  }
  *slot_out = (uint32_t)selection.code - 1u;
  return 1u;
}

/* the resident selection's activation: the published one when this card's
 * resident image still names it, else Base or Suspended from the file */
static RbActivation rb_activation_now(void)
{
  uint32_t code = (rb_loaded != 0u) ? rb_resident[RB_SELECTION_AT] : 0u;
  if (rb_media_matches(rb_activation_identity)) {
    if ((rb_activation.kind == RB_ACTIVATION_ACTIVE && code == rb_activation.slot + 1u) ||
        (rb_activation.kind == RB_ACTIVATION_BASE && code == 0u)) {
      return rb_activation;
    }
  }
  return (code == 0u) ? (RbActivation){RB_ACTIVATION_BASE, 0u}
                      : (RbActivation){RB_ACTIVATION_SUSPENDED, code - 1u};
}

bool rb_selection_effective(RbSelection *selection_out)
{
  RbActivation now;
  if (rb_loaded == 0u && rb_load_outcome != RB_NO_BANK && rb_load_outcome != RB_INVALID) {
    return false;
  }
  now = rb_activation_now();
  if (now.kind == RB_ACTIVATION_SUSPENDED) {
    return false;
  }
  *selection_out = (now.kind == RB_ACTIVATION_ACTIVE)
                       ? (RbSelection){(uint8_t)(now.slot + 1u)}
                       : RB_SELECTION_NONE;
  return true;
}

/* not at C and in the mode the transaction began in */
static uint32_t rb_select_gate(uint32_t mode)
{
  if (*(volatile uint32_t *)(uintptr_t)RB_MODE_CLASS == 3u) {
    return RB_AT_C;
  }
  return (*(volatile uint32_t *)(uintptr_t)RB_MODE_CELL == mode) ? RB_OK : RB_MODE_CHANGED;
}

/* every base field, family and ISO commit the target does not assert, without
 * 0x1c6 when a target commit resets it */
static void rb_select_rest(struct rb_apply *rest, const struct rb_apply *base,
                           const struct rb_apply *target)
{
  uint32_t f;
  rb_memcpy((uint8_t *)rest, (const uint8_t *)base, sizeof *rest);
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    const struct rb_field *field = &RB_FIELDS[f];
    if (rb_bit(target->skip, field) == 0u && rb_bit(rb_members, field) == 0u) {
      rb_set_bit(rest->skip, field);
    }
  }
  for (f = 0u; f < RB_FAMILY_COUNT; f++) {
    if (target->family[f] != RB_NONE) {
      rest->family[f] = RB_NONE;
    }
  }
  if (target->iso != RB_NONE) {
    rest->iso = RB_NONE;
  }
  if (rb_resets_1c6(target) != 0u) {
    rb_set_bit(rest->skip, rb_walked_field(RB_PROP_1C6));
  }
}

/* writes and verifies a generation of the resident image with the base block
 * or the selection byte replaced */
static uint32_t rb_select_write(const uint8_t *base, RbEligibleMode mode,
                                RbSelection selection, RbMediaIdentity identity)
{
  rb_stage_image();
  if (base != 0) {
    rb_memcpy(rb_staging + RB_BASE_AT, base, RB_PAYLOAD);
    rb_staging[RB_BASE_VALID_AT] = 1u;
    rb_staging[RB_BASE_MODE_AT] = (uint8_t)mode;
  }
  rb_staging[RB_SELECTION_AT] = selection.code;
  return rb_write_back(rb_current_gen + 1u, rb_slot_mask(rb_resident), identity);
}

/* One rollback owner: the live snapshot taken after the last I/O before any
 * live write is what every later failure re-applies. From Base the base is
 * captured from live and checkpointed with the selection unchanged before
 * anything is applied; from Active or Suspended the persisted base is applied
 * first, so the target's skipped fields take the base's codes, not the
 * outgoing bank's. The selection is written after the live assert, and its
 * failure restores the snapshot too. */
static uint32_t rb_select_run(RbSelection selection, RbMediaIdentity identity,
                              RbSelectTrace *trace)
{
  uint32_t mode = *(volatile uint32_t *)(uintptr_t)RB_MODE_CELL;
  RbEligibleMode eligible;
  RbActivation current;
  struct rb_apply base;
  struct rb_apply target;
  struct rb_apply rest;
  struct rb_apply restore;
  RbSelectTrace scratch;
  uint32_t slot = 0u;
  uint32_t custom = rb_selection_slot(selection, &slot);
  uint32_t refused = 0u;
  uint32_t status;
  rb_memset((uint8_t *)&scratch, 0u, sizeof scratch);
  trace->mode = mode;
  if (selection.code > RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  /* no bank operation writes the C record: at C the apply target resolves to
   * the slot-0 record */
  if (*(volatile uint32_t *)(uintptr_t)RB_MODE_CLASS == 3u) {
    return RB_AT_C;
  }
  if (rb_eligible_mode(mode, &eligible) == 0u) {
    return RB_APPLY_REFUSED;
  }
  if (rb_pending != RB_PENDING_NONE) {
    return RB_MODE_CHANGED;
  }
  status = rb_load_bound(identity);
  if (status == RB_NO_BANK || status == RB_INVALID) {
    return (custom != 0u) ? RB_UNSET : RB_OK;
  }
  if (status != RB_OK) {
    return status;
  }
  status = rb_select_gate(mode);
  if (status != RB_OK) {
    return status;
  }
  current = rb_activation_now();
  if (custom != 0u) {
    if ((rb_slot_mask(rb_resident) & (uint16_t)(1u << slot)) == 0u) {
      return RB_UNSET;
    }
    if (rb_codes_ok(rb_payload_at(rb_resident, slot)) == 0u ||
        rb_unbound_ok(rb_payload_at(rb_resident, slot)) == 0u) {
      return RB_INVALID;
    }
    if (current.kind == RB_ACTIVATION_ACTIVE && current.slot == slot) {
      return RB_OK;
    }
    target.payload = rb_payload_at(rb_resident, slot);
    if (rb_admit(&target, mode, trace) == 0u) {
      return RB_APPLY_REFUSED;
    }
  } else if (current.kind == RB_ACTIVATION_BASE) {
    return RB_OK;
  }
  if (current.kind == RB_ACTIVATION_BASE) {
    rb_payload_from_live(rb_select_base);
    if (rb_payload_safe(rb_select_base) == 0u) {
      return RB_INVALID;
    }
    status = rb_select_write(rb_select_base, eligible, RB_SELECTION_NONE, identity);
    if (status != RB_OK) {
      return status;
    }
    status = rb_select_gate(mode);
    if (status != RB_OK) {
      return status;
    }
    if (!rb_media_matches(identity)) {
      return RB_MEDIA_CHANGED;
    }
  } else {
    rb_memcpy(rb_select_base, rb_resident + RB_BASE_AT, RB_PAYLOAD);
    base.payload = rb_select_base;
    if (rb_admit_base(&base, mode, &scratch) == 0u) {
      return RB_INVALID;
    }
  }
  rb_snapshot_live(&restore, mode);
  if (current.kind != RB_ACTIVATION_BASE) {
    rb_apply_run(&base);
  }
  if (custom != 0u) {
    rb_apply_run(&target);
    refused = rb_assert_run(&target, trace);
  }
  if (current.kind != RB_ACTIVATION_BASE) {
    if (custom != 0u) {
      rb_select_rest(&rest, &base, &target);
      rb_reapply_run(&rest);
      refused += rb_assert_run(&rest, trace);
    } else {
      refused += rb_assert_run(&base, trace);
    }
  }
  if (refused != 0u) {
    rb_restore_run(&restore, trace);
    return RB_APPLY_REFUSED;
  }
  rb_observer_capture();
  status = rb_select_write(0, eligible, selection, identity);
  /* past this write a live write is safe only in the mode the transaction
   * began in: at C it would reach the C record, in another mode that mode's
   * family memories */
  if (status != RB_OK) {
    if (rb_select_gate(mode) != RB_OK) {
      rb_memcpy((uint8_t *)&rb_pending_restore, (const uint8_t *)&restore, sizeof restore);
      rb_pending_activation = current;
      rb_pending_identity = identity;
      rb_activation = (RbActivation){RB_ACTIVATION_SUSPENDED, current.slot};
      rb_pending = RB_PENDING_RESTORE;
      return RB_MODE_CHANGED;
    }
    rb_restore_run(&restore, trace);
    return status;
  }
  rb_activation = (custom != 0u) ? (RbActivation){RB_ACTIVATION_ACTIVE, slot}
                                 : (RbActivation){RB_ACTIVATION_BASE, 0u};
  rb_activation_identity = identity;
  if (rb_select_gate(mode) != RB_OK) {
    if (custom != 0u) {
      rb_activation = (RbActivation){RB_ACTIVATION_SUSPENDED, slot};
      rb_pending = RB_PENDING_BASE;
      rb_pending_identity = identity;
    }
    return RB_OK;
  }
  rb_observer_capture();
  return RB_OK;
}

static void
rb_trace_publish(uint32_t status, uint32_t selection, const RbSelectTrace *trace)
{
  if (status != RB_OK || trace->skipped_fields != 0u || trace->restore_refused != 0u) {
    rb_select_trace.status = status;
    rb_select_trace.selection = selection;
    rb_select_trace.prop = trace->prop;
    rb_select_trace.stored = trace->stored;
    rb_select_trace.read_back = trace->read_back;
    rb_select_trace.mode = trace->mode;
    rb_select_trace.refused_fields = trace->refused_fields;
    rb_select_trace.restore_refused = trace->restore_refused;
    rb_select_trace.skipped_fields = trace->skipped_fields;
    rb_select_trace.skipped_prop = trace->skipped_prop;
    rb_select_trace.count = rb_select_trace.count + 1u;
  }
}

uint32_t rb_select(RbSelection selection, RbMediaIdentity identity)
{
  RbSelectTrace trace;
  uint32_t status;
  rb_memset((uint8_t *)&trace, 0u, sizeof trace);
  rb_busy = 1u;
  status = rb_select_run(selection, identity, &trace);
  rb_busy = 0u;
  rb_trace_publish(status, selection.code, &trace);
  if (rb_pending != RB_PENDING_NONE) {
    (void)rb_mode_resume();
  }
  return status;
}

/* dial boundaries -------------------------------------------------------- */

static inline __attribute__((always_inline)) uint32_t rb_mode_class(void)
{
  return *(volatile uint32_t *)(uintptr_t)RB_MODE_CLASS;
}

/* the persisted base admitted in `mode`, applied and asserted: the number of
 * fields that do not read it back, or 1 when its admission refuses it */
static uint32_t rb_base_restore(uint32_t mode, RbSelectTrace *trace)
{
  struct rb_apply base;
  uint32_t refused;
  uint32_t f;
  base.payload = rb_resident + RB_BASE_AT;
  if (rb_admit_base(&base, mode, trace) == 0u) {
    return 1u;
  }
  rb_apply_run(&base);
  refused = rb_assert_run(&base, trace);
  if (rb_observer_valid == 0u) {
    return refused;
  }
  for (f = 0u; f < RB_FIELD_COUNT; f++) {
    const struct rb_field *field = &RB_FIELDS[f];
    uint32_t expected = rb_extract(base.payload, field);
    if (rb_extract(rb_observer_baseline, field) != expected) {
      refused += rb_assert_one(field->prop, expected, trace);
    }
  }
  return refused;
}

/* The outgoing overlay is removed before stock changes routing. */
uint32_t rb_mode_leave(uint32_t next_mode, uint32_t next_class)
{
  uint32_t mode = *(volatile uint32_t *)(uintptr_t)RB_MODE_CELL;
  uint32_t mode_class = rb_mode_class();
  RbEligibleMode eligible;
  RbActivation now;
  RbSelectTrace trace;
  struct rb_apply restore;
  uint32_t status = RB_OK;
  if (rb_busy != 0u) {
    return RB_MODE_CHANGED;
  }
  if (mode_class == 3u || rb_eligible_mode(mode, &eligible) == 0u ||
      (next_mode == mode && next_class == mode_class)) {
    return RB_OK;
  }
  if (rb_pending != RB_PENDING_NONE) {
    return RB_APPLY_REFUSED;
  }
  now = rb_activation_now();
  if (now.kind != RB_ACTIVATION_ACTIVE) {
    return RB_OK;
  }
  rb_memset((uint8_t *)&trace, 0u, sizeof trace);
  trace.mode = mode;
  rb_snapshot_live(&restore, mode);
  if (rb_base_restore(mode, &trace) != 0u) {
    status = RB_APPLY_REFUSED;
    rb_restore_run(&restore, &trace);
  }
  if (status == RB_OK || trace.restore_refused != 0u) {
    rb_activation = (RbActivation){RB_ACTIVATION_SUSPENDED, now.slot};
    if (status != RB_OK) {
      rb_pending = RB_PENDING_BASE;
      rb_pending_identity = rb_activation_identity;
    } else {
      rb_observer_valid = 0u;
    }
  }
  rb_trace_publish(status, now.slot + 1u, &trace);
  return status;
}

/* the remembered bank over a base freshly captured from the ordinary
 * settings: the base is checkpointed first when it differs from the
 * persisted one, then the bank is admitted in `mode`, applied and asserted,
 * and a refusal re-applies the live codes read before the apply */
static uint32_t rb_resume_run(uint32_t slot, RbEligibleMode eligible, uint32_t mode,
                              RbSelectTrace *trace)
{
  const uint8_t *payload = rb_payload_at(rb_resident, slot);
  struct rb_apply target;
  struct rb_apply restore;
  uint32_t status;
  if (rb_codes_ok(payload) == 0u || rb_unbound_ok(payload) == 0u) {
    return RB_INVALID;
  }
  rb_payload_from_live(rb_select_base);
  if (rb_payload_safe(rb_select_base) == 0u) {
    return RB_INVALID;
  }
  target.payload = payload;
  if (rb_admit(&target, mode, trace) == 0u) {
    return RB_APPLY_REFUSED;
  }
  if (rb_bytes_equal(rb_select_base, rb_resident + RB_BASE_AT, RB_PAYLOAD) == 0u) {
    status = rb_select_write(rb_select_base, eligible,
                             (RbSelection){(uint8_t)(slot + 1u)}, rb_activation_identity);
    if (status != RB_OK) {
      return status;
    }
    status = rb_select_gate(mode);
    if (status != RB_OK) {
      return status;
    }
  }
  rb_snapshot_live(&restore, mode);
  rb_apply_run(&target);
  status = rb_assert_run(&target, trace);
  if (status != 0u) {
    rb_restore_run(&restore, trace);
    status = RB_APPLY_REFUSED;
    if (trace->restore_refused != 0u) {
      rb_pending = RB_PENDING_BASE;
      rb_pending_identity = rb_activation_identity;
    }
  } else {
    rb_activation = (RbActivation){RB_ACTIVATION_ACTIVE, slot};
  }
  rb_observer_capture();
  return status;
}

/* Pending cleanup precedes ordinary-base capture and same-card resumption. */
uint32_t rb_mode_resume(void)
{
  uint32_t mode = *(volatile uint32_t *)(uintptr_t)RB_MODE_CELL;
  RbEligibleMode eligible;
  RbSelectTrace trace;
  RbPendingKind pending = rb_pending;
  uint32_t slot = rb_activation.slot;
  uint32_t status = RB_OK;
  if (rb_busy != 0u || rb_mode_class() == 3u || rb_eligible_mode(mode, &eligible) == 0u) {
    return RB_OK;
  }
  rb_memset((uint8_t *)&trace, 0u, sizeof trace);
  trace.mode = mode;
  if (pending != RB_PENDING_NONE) {
    uint32_t same_card = rb_media_matches(rb_pending_identity) ? 1u : 0u;
    if (pending == RB_PENDING_RESTORE && same_card != 0u &&
        rb_pending_restore.mode == mode) {
      rb_restore_run(&rb_pending_restore, &trace);
      status = (trace.restore_refused != 0u) ? RB_APPLY_REFUSED : RB_OK;
      if (status == RB_OK) {
        rb_activation = rb_pending_activation;
        rb_pending = RB_PENDING_NONE;
        rb_observer_capture();
      }
      goto done;
    }
    if (rb_base_restore(mode, &trace) != 0u) {
      status = RB_APPLY_REFUSED;
      goto done;
    }
    rb_pending = RB_PENDING_NONE;
    rb_observer_valid = 0u;
    if (same_card == 0u) {
      status = RB_MEDIA_CHANGED;
      goto done;
    }
  }
  if (rb_activation.kind != RB_ACTIVATION_SUSPENDED || rb_loaded == 0u ||
      !rb_media_matches(rb_activation_identity) ||
      rb_resident[RB_SELECTION_AT] != slot + 1u) {
    return RB_OK;
  }
  rb_busy = 1u;
  status = rb_resume_run(slot, eligible, mode, &trace);
  rb_busy = 0u;
done:
  rb_trace_publish(status, rb_resident[RB_SELECTION_AT], &trace);
  return status;
}

/* Replaces `bl FUN_00d1a7d8` at 0x00c2dfbc, the UI event 0x52 handler
 * 0x00c2dfb4 that every dial move in the shooting display and the menus
 * runs. FUN_00d1a7d8 takes the new class from 0x0094f654 and the new mode
 * from 0x0094f650 and returns the handler's verdict. */
uint32_t rb_mode_dial_apply(void)
{
  uint32_t (*stock)(void) = (uint32_t (*)(void))0x00d1a7d8u;
  uint32_t result;
  uint32_t status = rb_mode_leave(*(volatile uint32_t *)(uintptr_t)0x0094f650u,
                                  *(volatile uint32_t *)(uintptr_t)0x0094f654u);
  if (status != RB_OK) {
    rb_status_failed(status);
    return status;
  }
  result = stock();
  status = rb_mode_resume();
  if (status != RB_OK) {
    rb_status_failed(status);
  }
  return result;
}

/* Replaces `bl FUN_006439ac` at 0x00c2cf88 and 0x00c2d278, the handlers of
 * UI state 53 (table 0x00d03db8, events 2 and 0x11) that apply a dial move
 * made in playback: FUN_006439ac(mode, class) sets the class, then the mode. */
uint32_t rb_mode_pair_run(uint32_t next_mode, uint32_t next_class)
{
  void (*stock)(uint32_t, uint32_t) = (void (*)(uint32_t, uint32_t))0x006439acu;
  uint32_t status = rb_mode_leave(next_mode, next_class);
  if (status != RB_OK) {
    rb_status_failed(status);
    return status;
  }
  stock(next_mode, next_class);
  status = rb_mode_resume();
  if (status != RB_OK) {
    rb_status_failed(status);
  }
  return RB_OK;
}

/* Staged edit works over this kernel's own payload buffer and touches no
 * stock record or live setting: a staged field change is a validated bit
 * placement, and only commit or a selection writes anywhere. A set slot stages its
 * working payload. */
uint32_t rb_stage_load(uint32_t slot, RbMediaIdentity identity)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  status = rb_load_bound(identity);
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  if ((rb_slot_mask(rb_resident) & (uint16_t)(1u << slot)) != 0u) {
    rb_memcpy(rb_stage_payload, rb_payload_at(rb_resident, slot), RB_PAYLOAD);
    rb_memcpy(rb_stage_name, rb_name_at(rb_resident, slot), RB_NAME);
  } else {
    rb_payload_from_live(rb_stage_payload);
    rb_memset(rb_stage_name, 0u, RB_NAME);
  }
  rb_stage_slot = slot;
  rb_stage_identity = identity;
  return RB_OK;
}

/* the session's fields: the nine edit fields and the ISO auto flag */
static inline __attribute__((always_inline)) const struct rb_field *
rb_stage_field_of(uint32_t prop)
{
  uint32_t f;
  for (f = 0u; f < RB_EDIT_FIELD_COUNT; f++) {
    if (RB_EDIT_FIELDS[f].prop == prop) {
      return &RB_EDIT_FIELDS[f];
    }
  }
  return (prop == RB_ISO_AUTO_FLAG) ? rb_walked_field(prop) : 0;
}

uint32_t rb_stage_field(uint32_t prop, uint32_t code)
{
  const struct rb_field *f = rb_stage_field_of(prop);
  uint32_t staged;
  if (f == 0 || rb_code_ok(prop, code) == 0u) {
    return RB_RANGE;
  }
  staged = rb_extract(rb_stage_payload, f);
  rb_place(rb_stage_payload, f, code);
  /* a new ISO code takes the flag its commit arm writes, (row != 0x1b); a
   * snapshot taken in AUTO holds a numeric code with the flag at 0 */
  if (prop == RB_ISO_PROP && code != staged) {
    rb_place(rb_stage_payload, rb_stage_field_of(RB_ISO_AUTO_FLAG),
             (code != RB_ISO_AUTO) ? 1u : 0u);
  }
  return RB_OK;
}

uint32_t rb_stage_read(uint32_t prop)
{
  const struct rb_field *f = rb_stage_field_of(prop);
  return (f == 0) ? 0xffffffffu : rb_extract(rb_stage_payload, f);
}

void rb_stage_name_set(const uint8_t *name12)
{
  if (rb_name_ok(name12) != 0u) {
    rb_memcpy(rb_stage_name, name12, RB_NAME);
  }
}

/* A name-only generation write; a staged session for this same slot follows
 * the rename in its name field once the write has committed. An unset slot
 * takes the live settings under the name and becomes set; an empty name leaves
 * an unset slot unwritten. */
uint32_t rb_name_commit(uint32_t slot, const uint8_t *name26,
                        RbMediaIdentity identity)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  uint16_t bit = (uint16_t)(1u << slot);
  if (rb_name_ok(name26) == 0u) {
    return RB_RANGE;
  }
  status = rb_load_bound(identity);
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  rb_stage_image();
  if ((rb_slot_mask(rb_resident) & bit) == 0u) {
    uint8_t *seed = rb_saved_at(rb_staging, slot);
    if (name26[0] == 0u && name26[1] == 0u) {
      return RB_OK;
    }
    rb_payload_from_live(seed);
    if (rb_codes_ok(seed) == 0u) {
      return RB_INVALID;
    }
    rb_memcpy(rb_payload_at(rb_staging, slot), seed, RB_PAYLOAD);
  }
  rb_memcpy(rb_name_at(rb_staging, slot), name26, RB_NAME);
  status = rb_write_back(rb_current_gen + 1u,
                         (uint16_t)(rb_slot_mask(rb_resident) | bit), identity);
  if (status == RB_OK && rb_stage_slot == slot &&
      rb_stage_identity.epoch == identity.epoch) {
    rb_memcpy(rb_stage_name, name26, RB_NAME);
  }
  return status;
}

/* The staged payload becomes the working payload, and also the saved one under
 * ENABLE or for an unset slot. The session stays open after a commit;
 * rb_stage_cancel ends it. */
uint32_t rb_stage_commit(uint32_t slot, const uint8_t *name12)
{
  uint32_t status;
  const uint8_t *name = (name12 != 0u) ? name12 : rb_stage_name;
  if (slot >= RB_BANK_SLOTS || rb_stage_slot != slot) {
    return RB_RANGE;
  }
  if (rb_name_ok(name) == 0u) {
    return RB_RANGE;
  }
  if (rb_codes_ok(rb_stage_payload) == 0u || rb_unbound_ok(rb_stage_payload) == 0u) {
    return RB_INVALID;
  }
  status = rb_load_bound(rb_stage_identity);
  if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
    return status;
  }
  rb_stage_image();
  rb_memcpy(rb_payload_at(rb_staging, slot), rb_stage_payload, RB_PAYLOAD);
  if (rb_staging[RB_AUTO_AT] != 0u ||
      (rb_slot_mask(rb_resident) & (uint16_t)(1u << slot)) == 0u) {
    rb_memcpy(rb_saved_at(rb_staging, slot), rb_stage_payload, RB_PAYLOAD);
  }
  rb_memcpy(rb_name_at(rb_staging, slot), name, RB_NAME);
  status = rb_write_back(rb_current_gen + 1u,
                         (uint16_t)(rb_slot_mask(rb_resident) |
                                    (uint16_t)(1u << slot)), rb_stage_identity);
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

/* The bank state both verdicts read: the only request a call may make is the
 * read-only load a nonzero reload asks for. */
static uint32_t RB_DISCOVERY_IMPL(rb_meta_bank)(uint32_t reload RB_DISCOVERY_PARAMETER)
{
  uint32_t status;
  if (rb_task_ok() == 0u) {
    return RB_TASK_REFUSED;
  }
  if (reload != 0u) {
#ifdef RB_RAM_DIAGNOSTICS
    status = rb_load_bound_observed(rb_media_sample(), discovery);
    rb_discovery_load_end(discovery, (enum rb_bank_status)status,
                           (enum rb_bank_status)rb_load_outcome);
#else
    status = rb_load();
#endif
    if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
      return status;
    }
  } else if (rb_load_outcome != RB_OK && rb_load_outcome != RB_NO_BANK &&
             rb_load_outcome != RB_INVALID) {
    /* a failed discovery answers every cached verdict in the visit */
    return rb_load_outcome;
  }
#if defined(RB_VOLATILE_BANK) && RB_VOLATILE_BANK
  return RB_OK;
#else
  return rb_card_ready();
#endif
}

#ifdef RB_RAM_DIAGNOSTICS
static uint32_t rb_meta_bank(uint32_t reload)
{
  return rb_meta_bank_observed(reload RB_DISCOVERY_UNOBSERVED);
}
#endif

/* The verb pages' read-only verdict: whether a save of the slot would be
 * attempted. */
uint32_t rb_meta_save_state(uint32_t slot, uint32_t reload)
{
  uint32_t status;
  if (slot >= RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  status = rb_meta_bank(reload);
  if (status != RB_OK) {
    return status;
  }
  return rb_current_gen == 0xffffffffu ? RB_GEN_EXHAUST : RB_OK;
}

/* The availability probe's read-only verdict for one SELECT CUSTOM SETTING
 * row: outside P, S, A and M no row is selectable, NOTHING SELECTED is
 * selectable over any readable card, and CUSTOM n needs a populated slot in a
 * loaded bank. */
#ifdef RB_RAM_DIAGNOSTICS
static uint32_t rb_meta_select_state_impl(RbSelection selection, uint32_t reload,
                                          uint32_t mode, RbDiscoveryRequest discovery)
#else
uint32_t rb_meta_select_state(RbSelection selection, uint32_t reload)
#endif
{
#ifndef RB_RAM_DIAGNOSTICS
  uint32_t mode = *(volatile uint32_t *)(uintptr_t)RB_MODE_CELL;
#endif
  RbEligibleMode eligible;
  uint32_t slot;
  uint32_t status;
  if (selection.code > RB_BANK_SLOTS) {
    return RB_RANGE;
  }
  status = RB_DISCOVERY_IMPL(rb_meta_bank)(reload RB_DISCOVERY_ARGUMENT);
  if (status != RB_OK) {
    return status;
  }
  if (*(volatile uint32_t *)(uintptr_t)RB_MODE_CLASS == 3u) {
    return RB_AT_C;
  }
  if (rb_eligible_mode(mode, &eligible) == 0u) {
    return RB_APPLY_REFUSED;
  }
  if (rb_selection_slot(selection, &slot) == 0u) {
    return RB_OK;
  }
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

#ifdef RB_RAM_DIAGNOSTICS
uint32_t rb_meta_select_state(RbSelection selection, uint32_t reload,
                              RbDiagnosticWalk walk)
{
  uint32_t mode = *(volatile uint32_t *)(uintptr_t)RB_MODE_CELL;
  RbDiscoveryRequest discovery = {{0u}, {0u}, RB_DISCOVERY_NO_FILE};
  if (selection.code == 0u && reload != 0u) {
    discovery = rb_discovery_begin(walk);
  }
  uint32_t status = rb_meta_select_state_impl(selection, reload, mode, discovery);
  rb_discovery_end(discovery, (enum rb_bank_status)status);
  if (selection.code == 0u) {
    RbSelectNoneSource source = {
        .status = (enum rb_bank_status)status,
        .mode = {mode},
        .mode_class = (const volatile uint32_t *)(uintptr_t)RB_MODE_CLASS,
        .io_status = &rb_io_status,
        .io_value = &rb_io_value,
        .io_done = &rb_io_done};
    rb_select_diag_none(walk, source);
  } else {
    rb_select_diag_verdict(walk, selection, (enum rb_bank_status)status);
  }
  return status;
}
#endif

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
