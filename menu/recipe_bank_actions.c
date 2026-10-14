/* The bank's menu actions. Registration is four image-5 data writes (contract
 * section 1): action-table word 0xd04084 := rb_bank_interpose and three cells
 * (the browsing SET, the editor confirm, the leaving edge) := action index 1.
 * Rows the module does not claim leave through the stock handler of the state
 * and event it was entered from.
 */
#include "recipe_bank_ui.h"

#define RB_PAYLOAD 0x28u
#define RB_NAME 26u

static uint32_t rb_ui_word(uint32_t addr)
{
  return *(const volatile uint32_t *)addr;
}

static void rb_ui_store(uint32_t addr, uint32_t value)
{
  *(volatile uint32_t *)addr = value;
}

/* The values arrays end with {0x7ffffffe, 0}; a cursor at or past the
 * terminator is stale, which the framework's per-depth cursor can be when a
 * larger page's index is carried into a smaller child. */
static uint32_t rb_ui_row_ok(uint32_t values, uint32_t cursor)
{
  uint32_t i;
  if (values == 0u || cursor >= RB_UI_ROW_CAP) {
    return 0u;
  }
  for (i = 0u; i <= cursor; i++) {
    if (rb_ui_word(values + i * 8u) == 0x7ffffffeu) {
      return 0u;
    }
  }
  return 1u;
}

/* The stock child-enter pattern (byte-read in the state-0 classes that open
 * children), with the two pieces the framework leaves to the caller: the
 * child's parent field is repointed at the pushing frame, so the pop returns to
 * the page that pushed it, and the child's cursor starts at row 0 instead of
 * the depth record's leftover index. A refused transition puts the parent field
 * back, so a child whose emitted parent serves another caller keeps it. Entering
 * the slot list or the recall page republishes the seven slot names, so names
 * loaded from the card appear when the page is discovered. Returns the
 * transition result. */
uint32_t rb_ui_child_enter(void)
{
  uint32_t frame = rb_ui_word(RB_UI_FRAME_CELL);
  uint32_t values;
  uint32_t cursor;
  uint32_t child;
  uint32_t parent;
  uint32_t result;
  uint32_t cls;
  if (frame == 0u) {
    return 1u;
  }
  values = rb_ui_word(frame + RB_NODE_VALUES);
  cursor = rb_ui_word(RB_UI_CURSOR_CELL);
  if (rb_ui_row_ok(values, cursor) == 0u) {
    return 1u;
  }
  child = rb_ui_word(values + cursor * 8u + 4u);
  if (child == 0u) {
    return 1u;
  }
  parent = rb_ui_word(child + RB_NODE_PARENT);
  rb_ui_store(child + RB_NODE_PARENT, frame);
  rb_call_stage_open(0x0eu, 0x1du);
  result = rb_call_transition();
  if (result != 0xffffffffu && result != 2u) {
    rb_ui_store(child + RB_NODE_PARENT, parent);
    return result;
  }
  rb_ui_store(RB_UI_CURSOR_CELL, 0u);
  cls = rb_ui_word(child + RB_NODE_X);
  if (cls == RB_UI_CLASS_SLOT || cls == RB_UI_CLASS_RECALL) {
    uint32_t k;
    for (k = 0u; k < RB_BANK_SLOTS; k++) {
      (void)rb_slot_load(k);
    }
  }
  return result;
}

/* The modal state cells are halfwords; 0x1467620, the modal-76 state, is
 * adjacent to the modal-75 state at 0x146761e. The key-event cell is a
 * halfword as well (the dispatcher loads it with ldrh). */
static uint32_t rb_ui_state(void)
{
  return (uint32_t)*(const volatile uint16_t *)RB_UI_STATE_CELL;
}

static uint32_t rb_ui_event(void)
{
  return (uint32_t)*(const volatile uint16_t *)RB_UI_EVENT_CELL;
}

/* The stock confirm action FUN_00c75ed4 with the bank write in place of the
 * stock one, which is inert for the bank editor id. A refused write keeps the
 * editor open and reports the status; the success path returns the exit
 * transition result to the caller chain, whose -1 branch runs the parent-modal
 * bookkeeping. */
static uint32_t rb_ui_editor_commit(void)
{
  uint32_t status = rb_name_edit_commit();
  uint32_t result;
  if (status != (uint32_t)RB_OK) {
    rb_status_failed((enum rb_bank_status)status);
    return 1u;
  }
  rb_call_stage_open(0x0fu, 0x1du);
  result = rb_call_transition();
  if (result == 0xffffffffu || result == 2u) {
    rb_call_postlude(9u);
  }
  return result;
}

/* The stock handler of each cell this module is registered in; a further
 * registration must add its cell's handler here. */
static uint32_t rb_ui_stock(uint32_t state, uint32_t event)
{
  if (event == RB_UI_EVENT_LEAVE) {
    return (state == RB_UI_STATE_BROWSE) ? rb_call_leave_edge()
                                         : rb_call_set_dispatch();
  }
  if (state == RB_UI_STATE_EDITOR) {
    return rb_call_editor_confirm();
  }
  return rb_call_set_dispatch();
}

/* The stock delegate reads the selected row from the cursor cell with no bound
 * (FUN_00c72e10 -> FUN_0061d154 -> FUN_0061cd14 reads values[cursor]); a row
 * the values array does not carry is clamped to row 0 and the event is dropped. */
static uint32_t rb_ui_refuse_stale(void)
{
  rb_ui_store(RB_UI_CURSOR_CELL, 0u);
  return 1u;
}

uint32_t rb_bank_interpose(void)
{
  uint32_t frame = rb_ui_word(RB_UI_FRAME_CELL);
  uint32_t state = rb_ui_state();
  uint32_t event = rb_ui_event();
  uint32_t cls;
  uint32_t values;
  uint32_t cursor;
  uint32_t token;
  uint32_t child;
  uint8_t slot = 0u;
  RbBankPage page;
  if (frame == 0u) {
    return rb_ui_stock(state, event);
  }
  cls = rb_ui_word(frame + RB_NODE_X);
  if (event == RB_UI_EVENT_LEAVE) {
    if (cls == RB_UI_CLASS_EDIT) {
      uint32_t status = rb_stage_edit_leave();
      if (status == (uint32_t)RB_NOT_READY) {
        return rb_call_leave_edge();
      }
      if (status != (uint32_t)RB_OK) {
        rb_status_failed((enum rb_bank_status)status);
      }
      return 1u;
    }
    if (cls == RB_UI_CLASS_DIALOG) {
      (void)rb_stage_edit_dialog_cancel();
      return 1u;
    }
    if (cls >= RB_UI_CLASS_FIELD0 && cls <= RB_UI_CLASS_FIELD_LAST) {
      uint32_t status = rb_stage_edit_item_leave();
      if (status == (uint32_t)RB_NOT_READY) {
        return rb_call_leave_edge();
      }
      if (status != (uint32_t)RB_OK) {
        rb_status_failed((enum rb_bank_status)status);
      }
      return 1u;
    }
    return rb_ui_stock(state, event);
  }
  page = rb_ui_page(cls, &slot);
  if (page == RB_PAGE_EDITOR) {
    if (state == RB_UI_STATE_EDITOR &&
        rb_ui_word(frame + RB_NODE_ID) == RB_UI_EDITOR_ID) {
      values = rb_ui_word(frame + RB_NODE_VALUES);
      cursor = rb_ui_word(RB_UI_CURSOR_CELL);
      if (rb_ui_row_ok(values, cursor) == 0u) {
        return rb_ui_refuse_stale();
      }
      if (rb_ui_word(values + cursor * 8u) == RB_UI_EDITOR_CONFIRM_ROW) {
        return rb_ui_editor_commit();
      }
    }
    return rb_ui_stock(state, event);
  }
  if (state != RB_UI_STATE_BROWSE) {
    return rb_ui_stock(state, event);
  }
  values = rb_ui_word(frame + RB_NODE_VALUES);
  cursor = rb_ui_word(RB_UI_CURSOR_CELL);
  if (values == 0u) {
    /* The stock delegate's row read is guarded on the values word itself. */
    return rb_ui_stock(state, event);
  }
  if (rb_ui_row_ok(values, cursor) == 0u) {
    return rb_ui_refuse_stale();
  }
  token = rb_ui_word(values + cursor * 8u);
  child = rb_ui_word(values + cursor * 8u + 4u);
  if (page == RB_PAGE_NONE) {
    /* The two rows appended to the shooting root carry the bank's slot list and
     * recall page as their children and no stock branch claims their tokens, so
     * their pushes run here and take the parent repoint and the name
     * republication. Every other root row delegates unchanged. */
    if ((token == RB_UI_TOKEN_ITEM_EDIT_SAVE ||
         token == RB_UI_TOKEN_ITEM_RECALL) &&
        child != 0u) {
      uint32_t child_cls = rb_ui_word(child + RB_NODE_X);
      if (child_cls == RB_UI_CLASS_SLOT || child_cls == RB_UI_CLASS_RECALL) {
        return rb_ui_child_enter();
      }
    }
    return rb_ui_stock(state, event);
  }
  /* The hidden dialog placeholder row pushes its dialog child on any bank page
   * that carries it, so a leaked select cannot reach a row dispatch. */
  if (token == RB_UI_TOKEN_EDIT_HIDDEN_ROW && child != 0u) {
    return rb_ui_child_enter();
  }
  if (page == RB_PAGE_SLOT) {
    if (token >= RB_UI_TOKEN_SLOT_BASE &&
        token < RB_UI_TOKEN_SLOT_BASE + RB_BANK_SLOTS && child != 0u) {
      return rb_ui_child_enter();
    }
    return rb_ui_stock(state, event);
  }
  if (page == RB_PAGE_EDIT) {
    if (child != 0u) {
      return rb_ui_child_enter();
    }
    return rb_ui_stock(state, event);
  }
  if (page == RB_PAGE_DIALOG) {
    if (token == RB_UI_TOKEN_DIALOG_OK) {
      (void)rb_stage_edit_dialog_ok();
      return 1u;
    }
    if (token == RB_UI_TOKEN_DIALOG_CANCEL) {
      (void)rb_stage_edit_dialog_cancel();
      return 1u;
    }
    return rb_ui_stock(state, event);
  }
  if (page == RB_PAGE_FIELD) {
    uint32_t status = rb_stage_edit_row(cls, token);
    if (status != (uint32_t)RB_OK) {
      rb_status_failed((enum rb_bank_status)status);
    }
    return 1u;
  }
  if (page == RB_PAGE_RECALL) {
    if (token >= RB_UI_TOKEN_RECALL_BASE &&
        token < RB_UI_TOKEN_RECALL_BASE + RB_BANK_SLOTS) {
      (void)rb_bank_action(token);
      return 1u;
    }
    return rb_ui_stock(state, event);
  }
  if (token == rb_ui_verb_token(slot, RB_VERB_EDIT_NAME)) {
    uint32_t result;
    if (child == 0u) {
      return rb_ui_stock(state, event);
    }
    result = rb_ui_child_enter();
    if (result != 0xffffffffu && result != 2u) {
      return result;
    }
    (void)rb_bank_action(token);
    return result;
  }
  if (token == rb_ui_verb_token(slot, RB_VERB_SAVE_CURRENT) ||
      token == rb_ui_verb_token(slot, RB_VERB_RESET)) {
    uint32_t kind = (token == rb_ui_verb_token(slot, RB_VERB_SAVE_CURRENT))
                        ? (uint32_t)RB_CONFIRM_SAVE
                        : (uint32_t)RB_CONFIRM_RESET;
    uint32_t status =
        rb_stage_edit_confirm_begin((enum rb_edit_confirm)kind, slot);
    if (status != (uint32_t)RB_OK) {
      rb_status_failed((enum rb_bank_status)status);
    }
    return 1u;
  }
  if (token == rb_ui_verb_token(slot, RB_VERB_EDIT)) {
    (void)rb_bank_action(token);
    return 1u;
  }
  return rb_ui_stock(state, event);
}

/* Save Current keeps the slot's name; only the payload is replaced. */
static uint32_t rb_verb_save_current(uint8_t slot)
{
  uint8_t name[RB_NAME];
  uint8_t payload[RB_PAYLOAD];
  uint32_t status = rb_readback(slot, name, payload);
  if (status != (uint32_t)RB_OK) {
    return status;
  }
  status = rb_save_current(slot, name);
  if (status == (uint32_t)RB_OK) {
    rb_probe_invalidate();
  }
  return status;
}

/* The reset clears the stored name, so the resident label record follows it. */
static uint32_t rb_verb_reset(uint8_t slot)
{
  uint32_t status = rb_reset(slot);
  if (status == (uint32_t)RB_OK) {
    (void)rb_slot_load(slot);
    rb_probe_invalidate();
  }
  return status;
}

static uint32_t rb_verb_run(RbBankItemRef item)
{
  switch (item.kind) {
  case RB_ITEM_VERB:
    switch ((RbBankVerb)item.verb) {
    case RB_VERB_SAVE_CURRENT:
      return rb_verb_save_current(item.slot);
    case RB_VERB_EDIT_NAME:
      return rb_name_edit_open(item.slot);
    case RB_VERB_EDIT:
      return rb_stage_edit_open(item.slot);
    case RB_VERB_RESET:
      return rb_verb_reset(item.slot);
    }
    return (uint32_t)RB_RANGE;
  case RB_ITEM_RECALL:
    return rb_recall(item.slot);
  default:
    return (uint32_t)RB_RANGE;
  }
}

uint32_t rb_bank_action(uint32_t token)
{
  RbBankItemRef item = rb_ui_decode(token);
  uint32_t status;
  if (item.kind == RB_ITEM_NONE) {
    return 0u;
  }
  status = rb_verb_run(item);
  if (status != (uint32_t)RB_OK) {
    rb_status_failed((enum rb_bank_status)status);
  }
  return 1u;
}
