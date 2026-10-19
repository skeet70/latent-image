/* The nine-field edit surface (X-M1 1.50, candidate, non-installable).
 *
 * Entering Edit loads the selected slot into the storage kernel's session with
 * rb_stage_load. OK on a field row stores that row's code into the slot at once
 * (rb_stage_edit_row) and the interposer then hands the event to the stock SET
 * delegate, whose leaf exit pops the list and starts the stock blink. Leaving
 * the edit screen ends the session with nothing pending. No stock setter runs
 * and no live setting or C record byte moves.
 *
 * A field list's node X is its hosting property. Stock writes the property on
 * list entry and on every cursor move through the shared no-op commit arm
 * FUN_00d52664 and FUN_0061c91c; the setter dispatch word names
 * rb_stage_edit_set_arm, which returns 0 so FUN_006309c4's ordinary-block
 * store-back does not run. The getter dispatch word names rb_stage_edit_get,
 * which is what the row resolver and the highlight read.
 *
 * The Reset item confirms through the shared dialog: each verb page carries a
 * hidden dialog row whose child word names the dialog node.
 */
#include "recipe_bank_ui.h"
#include "recipe_bank_edit.h"

#define RB_EDIT_FIELD_COUNT 9u
#define RB_EDIT_SLOT_NONE 0xffffffffu

/* The nine fields: the stock property whose stored codes the payload carries,
 * the hosting property each field list carries as its X, and the pinned
 * table's row count, which bounds every table read. The hosting properties are
 * inert in the stock layer (shared no-op commit word, getter arm 0x6243e4
 * returning 0, setter arm 0x6301fc), sit outside the render probe's 5..0x260
 * jump table, and no dumped node carries any of them. */
static const struct {
  uint16_t prop;
  uint16_t host;
  uint8_t table_rows;
} RB_EDIT_FIELD[RB_EDIT_FIELD_COUNT] = {
    {0x104u, 0x267u, 6u},  {0x110u, 0x268u, 5u}, {0x111u, 0x269u, 5u},
    {0x10eu, 0x26au, 5u},  {0x10fu, 0x26bu, 5u}, {0xefu, 0x26cu, 5u},
    {0xedu, 0x26du, 11u},  {0x141u, 0x26eu, 28u}, {0xfeu, 0x26fu, 13u}};

static uint32_t rb_edit_slot = RB_EDIT_SLOT_NONE;
static uint32_t rb_edit_parent_row;
static uint32_t rb_reset_pending_slot = RB_EDIT_SLOT_NONE;
static RbMediaIdentity rb_confirm_identity;
static enum rb_bank_status rb_confirm_refused = RB_OK;

static uint32_t rb_ui_word(uint32_t addr)
{
  return *(const volatile uint32_t *)addr;
}

static void rb_ui_store_word(uint32_t addr, uint32_t value)
{
  *(volatile uint32_t *)addr = value;
}

static void rb_edit_fail(enum rb_bank_status status)
{
  rb_status_failed(status);
}

/* The stock confirm-exit pops one level. */
static void rb_edit_pop(void)
{
  uint32_t result;
  rb_call_stage_open(0x0fu, 0x1du);
  result = rb_call_transition();
  if (result == 0xffffffffu || result == 2u) {
    rb_ui_view_changed();
    rb_call_postlude(9u);
  }
}

static uint32_t rb_edit_staged(void)
{
  return (rb_edit_slot != RB_EDIT_SLOT_NONE) ? 1u : 0u;
}

static void rb_confirm_release(void)
{
  rb_reset_pending_slot = RB_EDIT_SLOT_NONE;
  rb_confirm_refused = RB_OK;
}

/* The EDIT verb's action; the push opens the edit screen whose rows are the
 * nine field lists. */
uint32_t rb_stage_edit_open(uint32_t slot, RbMediaIdentity identity)
{
  uint32_t status;
  uint32_t result;
  if (slot >= RB_BANK_SLOTS) {
    rb_edit_fail(RB_RANGE);
    return (uint32_t)RB_RANGE;
  }
  status = rb_stage_load(slot, identity);
  if (status != (uint32_t)RB_OK) {
    return status;
  }
  if (!rb_media_matches(identity)) {
    rb_stage_cancel();
    return RB_MEDIA_CHANGED;
  }
  result = rb_ui_child_enter();
  if (result != 0xffffffffu && result != 2u) {
    rb_stage_cancel();
    rb_edit_slot = RB_EDIT_SLOT_NONE;
    return (uint32_t)RB_NOT_READY;
  }
  rb_confirm_release();
  rb_edit_slot = slot;
  return (uint32_t)RB_OK;
}

/* The edit screen's DISP/BACK: every stored value was written by its OK. */
void rb_stage_edit_close(void)
{
  if (rb_edit_staged() != 0u) {
    rb_stage_cancel();
  }
  rb_edit_slot = RB_EDIT_SLOT_NONE;
}

/* The getter dispatch arm answers with the session's code. */
uint32_t rb_stage_edit_get(uint32_t host_minus_one)
{
  uint32_t f;
  if (rb_edit_staged() == 0u) {
    return 0u;
  }
  for (f = 0u; f < RB_EDIT_FIELD_COUNT; f++) {
    if (RB_EDIT_FIELD[f].host == host_minus_one + 1u) {
      return rb_stage_read(RB_EDIT_FIELD[f].prop);
    }
  }
  return 0u;
}

/* The pinned stock conversion tables of the nine fields; a row is 12 bytes and
 * its col0 word is the stored code. */
static const uint32_t RB_EDIT_TABLE[RB_EDIT_FIELD_COUNT] = {
    0x00775428u, 0x00775e24u, 0x0077643cu, 0x0077a810u, 0x00778620u,
    0x00779f10u, 0x0077a60cu, 0x0077af48u, 0x0077ac30u};

/* A field list's OK: the row's code is written to the slot before the list
 * closes, and a code equal to the stored one writes nothing. A row outside the
 * table's extent is refused before any memory access, and a code the table does
 * not carry is refused by rb_stage_field. A refused write puts the stored code
 * and the staged ISO auto flag back. Every refusal is reported here. */
uint32_t rb_stage_edit_row(uint32_t host, uint32_t row)
{
  uint32_t f;
  uint32_t stored;
  uint32_t stored_auto_flag;
  uint32_t code;
  uint32_t status;
  if (rb_edit_staged() == 0u) {
    rb_edit_fail(RB_NOT_READY);
    return (uint32_t)RB_NOT_READY;
  }
  for (f = 0u; f < RB_EDIT_FIELD_COUNT; f++) {
    if (RB_EDIT_FIELD[f].host == host) {
      break;
    }
  }
  if (f == RB_EDIT_FIELD_COUNT || row >= (uint32_t)RB_EDIT_FIELD[f].table_rows) {
    rb_edit_fail(RB_RANGE);
    return (uint32_t)RB_RANGE;
  }
  stored = rb_stage_read(RB_EDIT_FIELD[f].prop);
  stored_auto_flag = rb_stage_read(RB_ISO_AUTO_FLAG);
  code = rb_ui_word(RB_EDIT_TABLE[f] + 12u * row);
  status = rb_stage_field(RB_EDIT_FIELD[f].prop, code);
  if (status == (uint32_t)RB_OK && code != stored) {
    status = rb_stage_commit(rb_edit_slot, 0u);
    if (status != (uint32_t)RB_OK) {
      (void)rb_stage_field(RB_EDIT_FIELD[f].prop, stored);
      (void)rb_stage_field(RB_ISO_AUTO_FLAG, stored_auto_flag);
    } else {
      rb_probe_invalidate();
    }
  }
  if (status != (uint32_t)RB_OK) {
    rb_edit_fail((enum rb_bank_status)status);
  }
  return status;
}

void rb_bank_field_current_row(void)
{
  uint32_t frame = rb_ui_word(RB_UI_FRAME_CELL);
  uint32_t values;
  uint32_t host;
  uint32_t field;
  uint32_t row;
  uint32_t code;
  uint32_t i;
  if (frame == 0u || rb_edit_staged() == 0u) {
    return;
  }
  host = rb_ui_word(frame + RB_NODE_X);
  values = rb_ui_word(frame + RB_NODE_VALUES);
  if (values == 0u) {
    return;
  }
  for (field = 0u; field < RB_EDIT_FIELD_COUNT; field++) {
    if (RB_EDIT_FIELD[field].host == host) {
      break;
    }
  }
  if (field == RB_EDIT_FIELD_COUNT) {
    return;
  }
  code = rb_stage_read(RB_EDIT_FIELD[field].prop);
  for (row = 0u; row < (uint32_t)RB_EDIT_FIELD[field].table_rows; row++) {
    if (rb_ui_word(RB_EDIT_TABLE[field] + 12u * row) == code) {
      break;
    }
  }
  if (row == (uint32_t)RB_EDIT_FIELD[field].table_rows) {
    return;
  }
  for (i = 0u; i < RB_UI_ROW_CAP; i++) {
    uint32_t token = rb_ui_word(values + i * 8u);
    if (token == 0x7ffffffeu) {
      return;
    }
    if (token == row) {
      rb_ui_store_word(RB_UI_CURRENT_ROW_CELL, i);
      return;
    }
  }
}

/* Raised from a verb page; the cursor moves to the page's hidden dialog row
 * for the push and comes back on a refusal or when the dialog closes. */
uint32_t rb_stage_edit_reset_begin(uint32_t slot, RbMediaIdentity identity)
{
  uint32_t result;
  if (slot >= RB_BANK_SLOTS) {
    rb_edit_fail(RB_RANGE);
    return (uint32_t)RB_RANGE;
  }
  rb_reset_pending_slot = slot;
  rb_confirm_identity = identity;
  rb_confirm_refused = RB_OK;
  rb_edit_parent_row = rb_ui_word(RB_UI_CURSOR_CELL);
  rb_ui_store_word(RB_UI_CURSOR_CELL, RB_UI_VERBS_PER_SLOT);
  result = rb_ui_child_enter();
  if (result != 0xffffffffu && result != 2u) {
    rb_confirm_release();
    rb_ui_store_word(RB_UI_CURSOR_CELL, rb_edit_parent_row);
    return (uint32_t)RB_NOT_READY;
  }
  return (uint32_t)RB_OK;
}

static void rb_confirm_close(void)
{
  rb_confirm_release();
  rb_edit_pop();
  rb_ui_store_word(RB_UI_CURSOR_CELL, rb_edit_parent_row);
}

uint32_t rb_stage_edit_dialog_ok(void)
{
  uint32_t status;
  if (rb_confirm_refused != RB_OK || !rb_media_matches(rb_confirm_identity)) {
    rb_confirm_refused = RB_MEDIA_CHANGED;
    rb_edit_fail(RB_MEDIA_CHANGED);
    return RB_MEDIA_CHANGED;
  }
  if (rb_reset_pending_slot >= RB_BANK_SLOTS) {
    return (uint32_t)RB_RANGE;
  }
  status = (uint32_t)rb_bank_action(
      rb_ui_verb_token((uint8_t)rb_reset_pending_slot, RB_VERB_RESET),
      rb_confirm_identity);
  if (status != (uint32_t)RB_OK) {
    if (status == RB_MEDIA_CHANGED) {
      rb_confirm_refused = RB_MEDIA_CHANGED;
    }
    return status;
  }
  rb_confirm_close();
  return (uint32_t)RB_OK;
}

/* The dialog's CANCEL item and the dialog's own DISP/BACK. */
uint32_t rb_stage_edit_dialog_cancel(void)
{
  rb_confirm_close();
  return (uint32_t)RB_OK;
}
