/* The staged nine-field edit surface (X-M1 1.50, candidate, non-installable).
 *
 * Entering Edit stages the selected slot with rb_stage_load; each field row
 * commit stages one stored code with rb_stage_field; the leaving-edge dialog
 * commits with rb_stage_commit or discards with rb_stage_cancel, and serves the
 * Save Current and Reset confirmations. Between stage_load and commit/cancel
 * the only writes are the storage kernel's staging buffers: no stock setter
 * runs, and no live setting, C record or bank file byte moves before OK.
 *
 * A field list's node X is its hosting property, so a row commit arrives here
 * through the property layer: the shared no-op commit arm FUN_00d52664 writes
 * the property through FUN_0061c91c and the setter dispatch word names
 * rb_stage_edit_set, whose zero return keeps FUN_006309c4's ordinary-block
 * store-back from running. The getter dispatch word names rb_stage_edit_get,
 * which is what the row resolver and the highlight read.
 *
 * Page moves are the menu lane's rb_ui_child_enter for pushes and the stock
 * confirm-exit for pops; every page that can confirm carries a hidden dialog
 * row whose child word names the dialog node.
 */
#include "recipe_bank_ui.h"
#include "recipe_bank_edit.h"

#define RB_EDIT_FIELD_COUNT 9u
#define RB_EDIT_SLOT_NONE 0xffffffffu
#define RB_EDIT_DIALOG_ROW 9u

#define RB_EDIT_SCREEN_CLASS 0x277u

/* The nine fields: the stock property whose stored codes the payload carries,
 * the hosting property each field list carries as its X, the number of value
 * rows (the hidden dialog row follows them), and the pinned table's row count,
 * which bounds every table read. The hosting properties are inert in the stock
 * layer (shared no-op commit word, getter arm 0x6243e4 returning 0, setter arm
 * 0x6301fc), sit outside the render probe's 5..0x260 jump table, and no dumped
 * node carries any of them. */
static const struct {
  uint16_t prop;
  uint16_t host;
  uint8_t rows;
  uint8_t table_rows;
} RB_EDIT_FIELD[RB_EDIT_FIELD_COUNT] = {
    {0x104u, 0x267u, 4u, 6u},   {0x110u, 0x268u, 5u, 5u},
    {0x111u, 0x269u, 5u, 5u},   {0x10eu, 0x26au, 5u, 5u},
    {0x10fu, 0x26bu, 5u, 5u},   {0xefu,  0x26cu, 5u, 5u},
    {0xedu,  0x26du, 8u, 11u},  {0x141u, 0x26eu, 21u, 28u},
    {0xfeu,  0x26fu, 7u, 13u}};

static uint32_t rb_edit_slot = RB_EDIT_SLOT_NONE;
static uint32_t rb_edit_parent_row;
static uint32_t rb_edit_pending = (uint32_t)RB_CONFIRM_NONE;
static uint32_t rb_edit_pending_slot;
static uint32_t rb_item_host;
static uint32_t rb_item_saved;
static uint32_t rb_item_dirty;

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
    rb_call_postlude(9u);
  }
}

static uint32_t rb_edit_staged(void)
{
  return (rb_edit_slot != RB_EDIT_SLOT_NONE) ? 1u : 0u;
}

static void rb_edit_release(void)
{
  rb_edit_slot = RB_EDIT_SLOT_NONE;
  rb_edit_pending = (uint32_t)RB_CONFIRM_NONE;
  rb_edit_pending_slot = RB_EDIT_SLOT_NONE;
  rb_item_host = 0u;
  rb_item_saved = 0u;
  rb_item_dirty = 0u;
}

/* One item's confirmation ends the item session and nothing else: the staged
 * edit stays bound to its slot until the leaving-edge commit or cancel. */
static void rb_item_release(void)
{
  rb_item_host = 0u;
  rb_item_saved = 0u;
  rb_item_dirty = 0u;
  rb_edit_pending = (uint32_t)RB_CONFIRM_NONE;
  rb_edit_pending_slot = RB_EDIT_SLOT_NONE;
}

/* A resolved confirmation: the commit's dialog ends the staged edit session,
 * an item's dialog ends only the item session, and the item confirmations
 * clear their pending action. */
static void rb_confirm_resolved(void)
{
  if (rb_edit_pending == (uint32_t)RB_CONFIRM_COMMIT) {
    rb_edit_release();
    return;
  }
  if (rb_edit_pending == (uint32_t)RB_CONFIRM_ITEM) {
    rb_item_release();
    return;
  }
  rb_edit_pending = (uint32_t)RB_CONFIRM_NONE;
  rb_edit_pending_slot = RB_EDIT_SLOT_NONE;
}

static uint32_t rb_edit_frame_class(void);

/* The first staged change of one item's session remembers the code it
 * replaces. */
static void rb_item_snapshot(uint32_t host, uint32_t prop)
{
  if (rb_item_dirty != 0u && rb_item_host == host) {
    return;
  }
  if (rb_edit_frame_class() == host) {
    rb_item_saved = rb_stage_read(prop);
    rb_item_host = host;
    rb_item_dirty = 1u;
  }
}

static uint32_t rb_edit_host_prop(uint32_t host)
{
  uint32_t f;
  for (f = 0u; f < RB_EDIT_FIELD_COUNT; f++) {
    if (RB_EDIT_FIELD[f].host == host) {
      return RB_EDIT_FIELD[f].prop;
    }
  }
  return 0u;
}

static uint32_t rb_edit_frame_class(void)
{
  uint32_t frame = rb_ui_word(RB_UI_FRAME_CELL);
  return (frame != 0u) ? rb_ui_word(frame + RB_NODE_X) : 0u;
}

/* The hidden dialog row of the page the dialog is raised from. */
static uint32_t rb_edit_dialog_row(void)
{
  uint32_t cls = rb_edit_frame_class();
  uint32_t f;
  if (cls == RB_EDIT_SCREEN_CLASS) {
    return RB_EDIT_DIALOG_ROW;
  }
  if (cls >= RB_UI_CLASS_SUB0 && cls < RB_UI_CLASS_SUB0 + RB_BANK_SLOTS) {
    return RB_UI_VERBS_PER_SLOT;
  }
  for (f = 0u; f < RB_EDIT_FIELD_COUNT; f++) {
    if (RB_EDIT_FIELD[f].host == cls) {
      return RB_EDIT_FIELD[f].rows;
    }
  }
  return 0u;
}

/* The EDIT verb's action; the push opens the edit screen whose rows are the
 * nine field lists. */
uint32_t rb_stage_edit_open(uint32_t slot)
{
  uint32_t status;
  uint32_t result;
  if (slot >= RB_BANK_SLOTS) {
    rb_edit_fail(RB_RANGE);
    return (uint32_t)RB_RANGE;
  }
  status = rb_stage_load(slot);
  if (status != (uint32_t)RB_OK) {
    return status;
  }
  result = rb_ui_child_enter();
  if (result != 0xffffffffu && result != 2u) {
    rb_stage_cancel();
    rb_edit_release();
    return (uint32_t)RB_NOT_READY;
  }
  rb_edit_release();
  rb_edit_slot = slot;
  return (uint32_t)RB_OK;
}

/* The setter dispatch arm for the nine hosting properties: FUN_00624694
 * arrives with the property and the converted stored code. rb_stage_field
 * refuses an invalid code before it mutates the staged payload. */
uint32_t rb_stage_edit_set(uint32_t host, uint32_t code)
{
  uint32_t prop;
  uint32_t status;
  if (rb_edit_staged() == 0u) {
    return 0u;
  }
  prop = rb_edit_host_prop(host);
  if (prop == 0u) {
    return 0u;
  }
  rb_item_snapshot(host, prop);
  status = rb_stage_field(prop, code);
  if (status != (uint32_t)RB_OK) {
    rb_edit_fail((enum rb_bank_status)status);
  }
  return 0u;
}

/* The getter dispatch arm: the arm receives property - 1 and answers with the
 * staged code. */
uint32_t rb_stage_edit_get(uint32_t host_minus_one)
{
  uint32_t prop;
  if (rb_edit_staged() == 0u) {
    return 0u;
  }
  prop = rb_edit_host_prop(host_minus_one + 1u);
  if (prop == 0u) {
    return 0u;
  }
  return rb_stage_read(prop);
}

/* The row-token staging entry: the stored code is the col0 word of the row in
 * the pinned stock table. A row outside the table's extent is refused before
 * any memory access; a code the table does not carry is refused by
 * rb_stage_field before any mutation. */
uint32_t rb_stage_edit_row(uint32_t host, uint32_t row)
{
  static const uint32_t RB_EDIT_TABLE[RB_EDIT_FIELD_COUNT] = {
      0x00775428u, 0x00775e24u, 0x0077643cu, 0x0077a810u, 0x00778620u,
      0x00779f10u, 0x0077a60cu, 0x0077af48u, 0x0077ac30u};
  uint32_t f;
  uint32_t status;
  if (rb_edit_staged() == 0u) {
    return (uint32_t)RB_NOT_READY;
  }
  for (f = 0u; f < RB_EDIT_FIELD_COUNT; f++) {
    if (RB_EDIT_FIELD[f].host == host) {
      if (row >= (uint32_t)RB_EDIT_FIELD[f].table_rows) {
        rb_edit_fail(RB_RANGE);
        return (uint32_t)RB_RANGE;
      }
      rb_item_snapshot(host, RB_EDIT_FIELD[f].prop);
      status = rb_stage_field(RB_EDIT_FIELD[f].prop,
                              rb_ui_word(RB_EDIT_TABLE[f] + 12u * row));
      if (status != (uint32_t)RB_OK) {
        rb_edit_fail((enum rb_bank_status)status);
      }
      return status;
    }
  }
  return (uint32_t)RB_RANGE;
}

/* One parameter item's DISP/BACK: an edited item is confirmed by its own
 * OK/CANCEL dialog. An untouched item leaves through the stock pop, which the
 * caller runs when this returns RB_NOT_READY. */
uint32_t rb_stage_edit_item_leave(void)
{
  uint32_t cls = rb_edit_frame_class();
  if (rb_edit_staged() == 0u || rb_item_dirty == 0u ||
      rb_edit_host_prop(cls) == 0u || cls != rb_item_host) {
    return (uint32_t)RB_NOT_READY;
  }
  return rb_stage_edit_confirm_begin(RB_CONFIRM_ITEM, rb_edit_slot);
}

/* The DISP/BACK leaving edge of the edit screen; the dialog replaces the pop. */
uint32_t rb_stage_edit_leave(void)
{
  if (rb_edit_staged() == 0u) {
    return (uint32_t)RB_NOT_READY;
  }
  return rb_stage_edit_confirm_begin(RB_CONFIRM_COMMIT, rb_edit_slot);
}

/* The leaving edge carries RB_CONFIRM_COMMIT, one item's leaving edge carries
 * RB_CONFIRM_ITEM, and the Save Current and Reset items carry their own. */
uint32_t rb_stage_edit_confirm_begin(enum rb_edit_confirm what, uint32_t slot)
{
  uint32_t result;
  if (what != (uint32_t)RB_CONFIRM_COMMIT && slot >= RB_BANK_SLOTS) {
    rb_edit_fail(RB_RANGE);
    return (uint32_t)RB_RANGE;
  }
  if (what == (uint32_t)RB_CONFIRM_COMMIT && rb_edit_staged() == 0u) {
    return (uint32_t)RB_NOT_READY;
  }
  rb_edit_pending = (uint32_t)what;
  rb_edit_pending_slot = slot;
  rb_edit_parent_row = rb_ui_word(RB_UI_CURSOR_CELL);
  rb_ui_store_word(RB_UI_CURSOR_CELL, rb_edit_dialog_row());
  result = rb_ui_child_enter();
  if (result != 0xffffffffu && result != 2u) {
    rb_edit_pending = (uint32_t)RB_CONFIRM_NONE;
    rb_ui_store_word(RB_UI_CURSOR_CELL, rb_edit_parent_row);
    return (uint32_t)RB_NOT_READY;
  }
  return (uint32_t)RB_OK;
}

/* rb_stage_commit validates the whole staged payload before any write; one
 * item's OK keeps its staged change; the save and reset confirmations run
 * their item's bank action. */
uint32_t rb_stage_edit_dialog_ok(void)
{
  uint32_t status;
  uint32_t token;
  uint32_t pops = (rb_edit_pending == (uint32_t)RB_CONFIRM_SAVE ||
                   rb_edit_pending == (uint32_t)RB_CONFIRM_RESET)
                      ? 1u
                      : 2u;
  switch (rb_edit_pending) {
  case (uint32_t)RB_CONFIRM_COMMIT:
    status = rb_stage_commit(rb_edit_slot, 0u);
    if (status != (uint32_t)RB_OK) {
      rb_edit_fail((enum rb_bank_status)status);
      return status;
    }
    rb_probe_invalidate();
    break;
  case (uint32_t)RB_CONFIRM_ITEM:
    break;
  case (uint32_t)RB_CONFIRM_SAVE:
    token = rb_ui_verb_token((uint8_t)rb_edit_pending_slot, RB_VERB_SAVE_CURRENT);
    (void)rb_bank_action(token);
    break;
  case (uint32_t)RB_CONFIRM_RESET:
    token = rb_ui_verb_token((uint8_t)rb_edit_pending_slot, RB_VERB_RESET);
    (void)rb_bank_action(token);
    break;
  default:
    return (uint32_t)RB_RANGE;
  }
  rb_confirm_resolved();
  rb_edit_pop();
  if (pops == 2u) {
    rb_edit_pop();
  } else {
    rb_ui_store_word(RB_UI_CURSOR_CELL, rb_edit_parent_row);
  }
  return (uint32_t)RB_OK;
}

/* The dialog's CANCEL item and the dialog's own DISP/BACK: one item's CANCEL
 * puts back the code its session replaced; the leaving-edge CANCEL drops the
 * staging. The file, the live settings and the C record keep their pre-edit
 * bytes. */
uint32_t rb_stage_edit_dialog_cancel(void)
{
  uint32_t prop;
  uint32_t pops = (rb_edit_pending == (uint32_t)RB_CONFIRM_SAVE ||
                   rb_edit_pending == (uint32_t)RB_CONFIRM_RESET)
                      ? 1u
                      : 2u;
  if (rb_edit_pending == (uint32_t)RB_CONFIRM_COMMIT && rb_edit_staged() != 0u) {
    rb_stage_cancel();
  }
  if (rb_edit_pending == (uint32_t)RB_CONFIRM_ITEM && rb_item_dirty != 0u) {
    prop = rb_edit_host_prop(rb_item_host);
    if (prop != 0u) {
      (void)rb_stage_field(prop, rb_item_saved);
    }
  }
  rb_confirm_resolved();
  rb_edit_pop();
  if (pops == 2u) {
    rb_edit_pop();
  } else {
    rb_ui_store_word(RB_UI_CURSOR_CELL, rb_edit_parent_row);
  }
  return (uint32_t)RB_OK;
}
