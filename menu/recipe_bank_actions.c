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

/* rb_call_preview_clear is FUN_00635944(0), which clears 0x009504ac; while
 * that cell is 1, FUN_0110871c consumes 0x40f1 without drawing. 0x400 becomes
 * packet 0x40f1; its stock callback selects resolver root 1. */
static void rb_ui_redraw_page(RbBankPage page)
{
  rb_call_preview_clear();
  rb_call_redraw(page == RB_PAGE_EDITOR ? 0x148u : 0x400u);
}

void rb_bank_present(uint32_t event)
{
  uint32_t frame = rb_ui_word(RB_UI_FRAME_CELL);
  uint32_t cls = frame == 0u ? 0u : rb_ui_word(frame + RB_NODE_X);
  uint8_t slot;
  RbBankPage page = rb_ui_page(cls, &slot);
  if (event != 0x816u || page == RB_PAGE_NONE ||
      (page == RB_PAGE_EDITOR && rb_ui_word(frame + RB_NODE_ID) != RB_UI_EDITOR_ID)) {
    rb_call_stock_present(event);
    return;
  }
  rb_ui_redraw_page(page);
  rb_ui_store(0x01467a08u, 0u);
}

/* FUN_011662a0 draws neither its full-screen backdrop nor the left pane for a
 * depth-2 frame whose node type is 9 (FUN_01165aa0 sets 0x015e64dc = 3 and
 * 0x015e64e8 = 1). The verb pages carry type 9; this is the backdrop call
 * FUN_011662a0 makes when 0x015e64e8 is 0, under the same FUN_01135db0 gate and
 * in the same layer 0x12. */
static void rb_bank_draw_backdrop(void)
{
  if (rb_call_backdrop_suppressed() != 0u) {
    return;
  }
  uint32_t saved_mode = rb_ui_word(0x015e46c0u);
  rb_call_draw_mode(0x12u);
  rb_call_name_rect(1, 1, 0x20, 0x14, 0x12u, 0u, 0xffu);
  rb_call_draw_mode(saved_mode);
}

/* Runs in the graphics task, after the copied display packet is consumed. A
 * field list shares the edit screen's root; at snapshot depth 2 FUN_011662a0
 * draws the edit rows with the open list beside the row whose child class is
 * the list's X, and the list's marker from RB_UI_CURRENT_ROW_CELL. */
void rb_bank_draw_root(uint32_t root)
{
  uint32_t frame = rb_ui_word(RB_UI_FRAME_CELL);
  uint8_t slot;
  RbBankPage page = frame == 0u ? RB_PAGE_NONE
      : rb_ui_page(rb_ui_word(frame + RB_NODE_X), &slot);
  if (page == RB_PAGE_DIALOG) {
    rb_call_draw_root_prepare(rb_ui_word(0x015e4894u));
    rb_call_draw_action_list((RbMenuRoot){rb_ui_word(frame + RB_NODE_ID)});
    return;
  }
  if (page == RB_PAGE_FIELD) {
    rb_bank_field_current_row();
  }
  if (page == RB_PAGE_SUB) {
    /* FUN_0115815c with animation kind 0: FUN_01166200 starts the pending
     * preparation (the entry slide 9 or the SET blink 7), and FUN_01165aa0's
     * default branch reads the depth-2/type-9 snapshot and sets 0x015e64dc = 3.
     * Its cases 7 and 9, which kind 0x015e489c selects while that preparation
     * runs, leave 0x015e64e4 = 0 and 0x015e64dc below 3, for which
     * FUN_011662a0 draws the left pane's bar FUN_0111236c(5, ..., 0xff0000f3). */
    rb_bank_draw_backdrop();
    rb_call_draw_root_prepare(rb_ui_word(0x015e4894u));
    rb_call_draw_pane(rb_ui_word(frame + RB_NODE_ID), 0u);
    return;
  }
  if (page == RB_PAGE_SLOT || page == RB_PAGE_RECALL ||
      page == RB_PAGE_EDIT || page == RB_PAGE_FIELD) {
    rb_call_stock_draw_root(rb_ui_word(frame + RB_NODE_ID));
    return;
  }
  rb_call_stock_draw_root(root);
}

typedef struct {
  uint32_t value;
} RbNameGlyph;

typedef struct {
  uint32_t value;
} RbNameLabel;

typedef enum {
  RB_NAME_ICON = 2,
  RB_NAME_LABEL = 3,
  RB_NAME_TEXT = 4
} RbNameCellKind;

typedef enum {
  RB_NAME_SELECT_NARROW0,
  RB_NAME_SELECT_NARROW1,
  RB_NAME_SELECT_NARROW2,
  RB_NAME_SELECT_WIDE0,
  RB_NAME_SELECT_WIDE1,
  RB_NAME_SELECT_LONG,
  RB_NAME_SELECT_SHORT
} RbNameSelection;

typedef struct {
  RbNameCellKind kind;
  uint16_t text[4];
  RbNameGlyph glyph;
  RbNameLabel label;
  int8_t x;
  int8_t y;
  uint8_t selection;
  uint8_t reserved;
} RbNameCell;

_Static_assert(sizeof(RbNameCell) == 24u, "Stock keyboard descriptor size");

typedef struct {
  uint8_t value;
} RbNameCursor;

typedef struct {
  uint8_t value;
} RbNameCellIndex;

typedef struct {
  uint32_t value;
} RbNameStyle;

typedef struct {
  uint32_t value;
} RbNameDrawMode;

typedef struct {
  char text[15];
  RbNameCursor cursor;
  RbNameCellIndex selected;
} RbNameSnapshot;

/* FUN_01115bf8 uses buffer A and the 43 descriptors at 0x0110d008. */
void rb_bank_draw_name(void)
{
  const RbNameCell *cells = (const RbNameCell *)0x0110d008u;
  const volatile char *buffer = (const volatile char *)0x00954385u;
  RbNameSnapshot name;
  name.cursor.value = *(const volatile uint8_t *)0x00954384u;
  name.selected.value = (uint8_t)rb_ui_word(RB_UI_CURSOR_CELL);
  bool terminated = false;
  for (uint32_t i = 0u; i < sizeof(name.text) - 1u; ++i) {
    char unit = buffer[i];
    terminated = terminated || unit == '\0';
    name.text[i] = terminated ? ' ' : unit;
  }
  name.text[sizeof(name.text) - 1u] = '\0';
  RbNameDrawMode saved_mode = {rb_ui_word(0x015e46c0u)};
  RbNameStyle style = {rb_call_name_style(0u)};
  if (rb_ui_word(0x015e4894u) == 4u) {
    rb_call_name_background(5u);
  }
  rb_call_name_title(0x197u, 0u);
  rb_call_draw_mode(0x12u);
  rb_call_name_rect(3, 2, 0x20, 0x13, 0x11u, 0x68u, 0xffu);
  rb_call_name_shell();
  rb_call_draw_mode(0x11u);
  rb_call_name_label(4, 4, 0xff000000u);
  rb_call_name_material(0x6au, 0xffu, 0u, 0u);
  rb_call_draw_mode(0xfu);
  for (uint32_t i = 0u; i < 43u; ++i) {
    const RbNameCell *cell = &cells[i];
    if (cell->kind == RB_NAME_TEXT) {
      rb_call_name_utf16(cell->x, cell->y, cell->text);
    } else if (cell->kind == RB_NAME_ICON) {
      rb_call_name_label(cell->x, cell->y, cell->glyph.value);
    } else if (cell->kind == RB_NAME_LABEL) {
      int8_t x;
      int8_t y;
      rb_call_name_label_position(cell->label.value, &x, &y);
      rb_call_name_label(x, y, cell->label.value);
    }
  }
  const RbNameCell *selected = &cells[name.selected.value];
  RbNameSelection selection = (RbNameSelection)selected->selection;
  rb_call_draw_mode(0x10u);
  if (selection <= RB_NAME_SELECT_NARROW2) {
    const RbNameGlyph *glyphs = (const RbNameGlyph *)0x0110915cu
        + style.value * 9u + (uint32_t)selection * 3u;
    rb_call_name_label(selected->x - 1, selected->y, glyphs[0].value);
    rb_call_name_label(selected->x, selected->y, glyphs[1].value);
    rb_call_name_label(selected->x + 1, selected->y, glyphs[2].value);
  } else if (selection <= RB_NAME_SELECT_WIDE1) {
    const RbNameGlyph *glyphs = (const RbNameGlyph *)0x0110e548u
        + style.value * 9u + ((uint32_t)selection - RB_NAME_SELECT_WIDE0) * 3u;
    rb_call_name_label(selected->x, selected->y, glyphs[0].value);
    rb_call_name_label(selected->x + 1, selected->y, glyphs[1].value);
    rb_call_name_label(selected->x + 2, selected->y, glyphs[2].value);
  } else if (selection == RB_NAME_SELECT_LONG ||
             selection == RB_NAME_SELECT_SHORT) {
    bool long_selection = selection == RB_NAME_SELECT_LONG;
    const RbNameGlyph *glyphs = (const RbNameGlyph *)(long_selection
        ? 0x0110e560u : 0x01109168u) + style.value * 9u;
    int8_t end = (int8_t)(selected->x + (long_selection ? 15 : 2));
    rb_call_name_label(selected->x - 1, selected->y, glyphs[0].value);
    rb_call_name_stretch((uint32_t)selected->x, end,
                         (uint32_t)selected->y, 1, 2, glyphs[1].value);
    rb_call_name_label(end + 1, selected->y, glyphs[2].value);
  }
  rb_call_draw_mode(0xeu);
  rb_call_name_material(0u, 0xffu, 0u, 0u);
  if (selected->kind == RB_NAME_TEXT) {
    rb_call_name_utf16(selected->x, selected->y, selected->text);
  } else if (selected->kind == RB_NAME_ICON) {
    rb_call_name_label(selected->x, selected->y, selected->glyph.value);
  } else if (selected->kind == RB_NAME_LABEL) {
    int8_t x;
    int8_t y;
    rb_call_name_label_position(selected->label.value, &x, &y);
    rb_call_name_label(x, y, selected->label.value);
  }
  rb_call_draw_mode(0xfu);
  rb_call_name_material(0u, 0xffu, 0u, 0u);
  rb_call_name_ascii(0xf, 5, name.text);
  int32_t cursor_x = name.cursor.value + 0xf;
  rb_call_name_material(0u, 0xffu, 0u, 0u);
  rb_call_name_label(cursor_x, 5, 0xff0004c2u);
  rb_call_draw_mode(0xeu);
  name.text[0] = name.text[name.cursor.value];
  name.text[1] = '\0';
  rb_call_name_material(0u, 0xffu, 0u, 0u);
  rb_call_name_label(cursor_x, 5, 0xff0004c2u);
  rb_call_draw_mode(0xdu);
  rb_call_name_material(7u, 0xffu, 0u, 0u);
  rb_call_name_ascii(cursor_x, 5, name.text);
  rb_call_draw_mode(0xfu);
  rb_call_name_footer(0x485u);
  rb_call_draw_mode(saved_mode.value);
}

typedef struct {
  uint32_t address;
} RbMenuFrame;

static RbMenuFrame rb_verb_frame;
static RbMediaIdentity rb_verb_identity;
static uint32_t rb_verb_class;
static uint32_t rb_verb_status = RB_NOT_READY;

void rb_ui_probe_visit(void)
{
  uint32_t frame = rb_ui_word(RB_UI_FRAME_CELL);
  rb_probe_invalidate();
  uint32_t cls = frame == 0u ? 0u : rb_ui_word(frame + RB_NODE_X);
  if (frame != rb_verb_frame.address || cls != rb_verb_class) {
    rb_ui_view_changed();
    rb_verb_frame.address = frame;
    rb_verb_class = cls;
    rb_verb_identity = rb_media_sample();
    rb_verb_status = RB_NOT_READY;
  } else if (rb_verb_status == RB_MEDIA_CHANGED) {
    return;
  }
  if (!rb_media_matches(rb_verb_identity)) {
    rb_verb_status = RB_MEDIA_CHANGED;
    return;
  }
  if (rb_verb_class >= RB_UI_CLASS_SUB0 &&
      rb_verb_class < RB_UI_CLASS_SUB0 + RB_BANK_SLOTS) {
    rb_verb_status = rb_meta_op_state(RB_BANK_SAVE,
        rb_verb_class - RB_UI_CLASS_SUB0, 1u);
  }
  if (!rb_media_matches(rb_verb_identity)) {
    rb_verb_status = RB_MEDIA_CHANGED;
  }
}

enum rb_row_state rb_ui_probe_state(uint32_t property, uint32_t row)
{
  enum rb_row_state state = rb_probe_state(property, row);
  if (property < RB_UI_CLASS_SUB0 ||
      property >= RB_UI_CLASS_SUB0 + RB_BANK_SLOTS) {
    return state;
  }
  uint32_t first = rb_ui_verb_token(property - RB_UI_CLASS_SUB0,
                                   RB_VERB_SAVE_CURRENT);
  /* Stock discovery pops a page whose rows are all disabled or hidden. */
  if (row == first) {
    return RB_ROW_ACTIVE;
  }
  if (state != RB_ROW_ACTIVE) {
    return state;
  }
  if (row >= first && row < first + RB_UI_VERBS_PER_SLOT) {
    return rb_verb_class == property && rb_media_matches(rb_verb_identity) &&
           rb_verb_status == RB_OK ? RB_ROW_ACTIVE : RB_ROW_DISABLED;
  }
  return state;
}

static uint32_t rb_ui_state(void);
static uint32_t rb_ui_pop(void)
{
  uint32_t editor = rb_ui_state() == RB_UI_STATE_EDITOR;
  rb_call_stage_open(0x0fu, 0x1du);
  uint32_t result = rb_call_transition();
  if (result == 0xffffffffu || result == 2u) {
    rb_ui_view_changed();
  }
  if (editor != 0u && (result == 0xffffffffu || result == 2u)) {
    *(volatile uint16_t *)RB_UI_STATE_CELL = RB_UI_STATE_BROWSE;
    rb_call_postlude(9u);
  }
  return result;
}

static uint32_t rb_ui_row_ok(uint32_t values, uint32_t cursor);

uint32_t rb_bank_key_right(void)
{
  uint32_t frame = rb_ui_word(RB_UI_FRAME_CELL);
  uint8_t slot;
  RbBankPage page = frame == 0u ? RB_PAGE_NONE
      : rb_ui_page(rb_ui_word(frame + RB_NODE_X), &slot);
  if (page == RB_PAGE_NONE && frame == 0x0078ea98u) {
    uint32_t values = rb_ui_word(frame + RB_NODE_VALUES);
    uint32_t cursor = rb_ui_word(RB_UI_CURSOR_CELL);
    if (rb_ui_row_ok(values, cursor) != 0u) {
      uint32_t token = rb_ui_word(values + cursor * 8u);
      if (token == RB_UI_TOKEN_ITEM_EDIT_SAVE || token == RB_UI_TOKEN_ITEM_RECALL) {
        return rb_bank_interpose();
      }
    }
  }
  if (page == RB_PAGE_NONE ||
      (page == RB_PAGE_EDITOR && rb_ui_word(frame + RB_NODE_ID) != RB_UI_EDITOR_ID)) {
    return rb_call_stock_key_right();
  }
  /* FUN_00c70528 leaves an open value list without a commit or redraw. */
  if (page == RB_PAGE_EDITOR || page == RB_PAGE_FIELD) {
    return rb_call_stock_key_right();
  }
  return rb_bank_interpose();
}

uint32_t rb_bank_key_left(void)
{
  uint32_t frame = rb_ui_word(RB_UI_FRAME_CELL);
  uint8_t slot;
  RbBankPage page = frame == 0u ? RB_PAGE_NONE
      : rb_ui_page(rb_ui_word(frame + RB_NODE_X), &slot);
  if (page == RB_PAGE_NONE ||
      (page == RB_PAGE_EDITOR && rb_ui_word(frame + RB_NODE_ID) != RB_UI_EDITOR_ID)) {
    return rb_call_stock_key_left();
  }
  if (page == RB_PAGE_EDITOR) {
    return rb_call_stock_key_left();
  }
  if (page == RB_PAGE_DIALOG) {
    (void)rb_stage_edit_dialog_cancel();
    return 1u;
  }
  if (page == RB_PAGE_EDIT) {
    rb_stage_edit_close();
  }
  return rb_ui_pop();
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
 * the depth record's leftover index. A field list keeps the cursor the page
 * walk FUN_00bfa800 placed on the getter's current row, with the raw row
 * 0x009571d8 and visible ordinal 0x009571dc both set there; a row the list does
 * not carry falls back to row 0. A refused transition puts the parent field
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
  rb_ui_view_changed();
  cls = rb_ui_word(child + RB_NODE_X);
  cursor = rb_ui_word(RB_UI_CURSOR_CELL);
  values = rb_ui_word(child + RB_NODE_VALUES);
  if (cls < RB_UI_CLASS_FIELD0 || cls > RB_UI_CLASS_FIELD_LAST ||
      rb_ui_row_ok(values, cursor) == 0u) {
    rb_ui_store(RB_UI_CURSOR_CELL, 0u);
    if (cls == RB_UI_CLASS_DIALOG ||
        (cls >= RB_UI_CLASS_FIELD0 && cls <= RB_UI_CLASS_FIELD_LAST)) {
      rb_ui_store(RB_UI_VISIBLE_CURSOR_CELL, 0u);
    }
  }
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
  if (status != (uint32_t)RB_OK) {
    rb_status_failed((enum rb_bank_status)status);
    return 1u;
  }
  return rb_ui_pop();
}

/* The stock handler of each cell this module is registered in; a further
 * registration must add its cell's handler here. */
static uint32_t rb_ui_stock(uint32_t state, uint32_t event)
{
  if (event == RB_UI_EVENT_LEAVE) {
    if (state == RB_UI_STATE_EDITOR) {
      return rb_call_editor_cancel();
    }
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
  page = rb_ui_page(cls, &slot);
  if (page == RB_PAGE_EDITOR && rb_ui_word(frame + RB_NODE_ID) != RB_UI_EDITOR_ID) {
    return rb_ui_stock(state, event);
  }
  if (event == RB_UI_EVENT_LEAVE) {
    if (cls == RB_UI_CLASS_EDIT) {
      rb_stage_edit_close();
      return rb_ui_pop();
    }
    if (cls == RB_UI_CLASS_DIALOG) {
      (void)rb_stage_edit_dialog_cancel();
      return 1u;
    }
    if (page == RB_PAGE_EDITOR) {
      (void)rb_name_edit_cancel();
      return rb_ui_pop();
    }
    if (page != RB_PAGE_NONE) {
      return rb_ui_pop();
    }
    return rb_ui_stock(state, event);
  }
  if (page == RB_PAGE_EDITOR) {
    if (state == RB_UI_STATE_EDITOR &&
        rb_ui_word(frame + RB_NODE_ID) == RB_UI_EDITOR_ID) {
      values = rb_ui_word(frame + RB_NODE_VALUES);
      cursor = rb_ui_word(RB_UI_CURSOR_CELL);
      if (rb_ui_row_ok(values, cursor) == 0u) {
        return rb_ui_refuse_stale();
      }
      token = rb_ui_word(values + cursor * 8u);
      if (token == RB_UI_EDITOR_CONFIRM_ROW) {
        return rb_ui_editor_commit();
      }
      /* FUN_00d4a2a0 drops an insert while buffer A's fourteenth byte is set
       * and draws nothing; the bank's name ends at twelve characters. */
      if (token < RB_UI_EDITOR_MOVE_LEFT_ROW && rb_name_edit_full()) {
        return 1u;
      }
    }
    return rb_ui_stock(state, event);
  }
  if (state != RB_UI_STATE_BROWSE) {
    return rb_ui_stock(state, event);
  }
  values = rb_ui_word(frame + RB_NODE_VALUES);
  cursor = rb_ui_word(RB_UI_CURSOR_CELL);
  if (page == RB_PAGE_NONE) {
    if (frame == 0x0078ea98u && rb_ui_row_ok(values, cursor) != 0u) {
      token = rb_ui_word(values + cursor * 8u);
      child = rb_ui_word(values + cursor * 8u + 4u);
      if ((token == RB_UI_TOKEN_ITEM_EDIT_SAVE || token == RB_UI_TOKEN_ITEM_RECALL) &&
          child != 0u) {
        uint32_t child_cls = rb_ui_word(child + RB_NODE_X);
        if (child_cls == RB_UI_CLASS_SLOT || child_cls == RB_UI_CLASS_RECALL) {
          return rb_ui_child_enter();
        }
      }
    }
    return rb_ui_stock(state, event);
  }
  if (values == 0u) {
    /* The stock delegate's row read is guarded on the values word itself. */
    return rb_ui_stock(state, event);
  }
  if (rb_ui_row_ok(values, cursor) == 0u) {
    return rb_ui_refuse_stale();
  }
  token = rb_ui_word(values + cursor * 8u);
  child = rb_ui_word(values + cursor * 8u + 4u);
  if (token == RB_UI_TOKEN_EDIT_HIDDEN_ROW) {
    return 1u;
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
  /* OK stores the row's code, then the stock SET delegate's leaf exit pops the
   * list, requests 0x930 and sets 0x009504a8. */
  if (page == RB_PAGE_FIELD) {
    uint32_t result;
    if (rb_stage_edit_row(cls, token) != (uint32_t)RB_OK) {
      return 1u;
    }
    result = rb_ui_stock(state, event);
    if (result == 0xffffffffu || result == 2u) {
      rb_ui_view_changed();
    }
    return result;
  }
  /* OK applies the slot, then the stock SET delegate's leaf exit pops to the
   * shooting menu, requests 0x930 and sets 0x009504a8. A refused recall has
   * reported its status and stays on the list. */
  if (page == RB_PAGE_RECALL) {
    uint32_t result;
    if (token < RB_UI_TOKEN_RECALL_BASE ||
        token >= RB_UI_TOKEN_RECALL_BASE + RB_BANK_SLOTS) {
      return rb_ui_stock(state, event);
    }
    if (rb_bank_action(token, rb_verb_identity) != RB_OK) {
      return 1u;
    }
    result = rb_ui_stock(state, event);
    if (result == 0xffffffffu || result == 2u) {
      rb_ui_view_changed();
    }
    return result;
  }
  RbMediaIdentity identity = rb_verb_identity;
  if (rb_verb_status != RB_OK || !rb_media_matches(identity) ||
      rb_probe_state(cls, token) != RB_ROW_ACTIVE) {
    rb_status_failed(rb_media_matches(identity)
        ? (enum rb_bank_status)rb_verb_status : RB_MEDIA_CHANGED);
    return 1u;
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
    enum rb_bank_status status =
        (enum rb_bank_status)rb_name_edit_open(slot, identity);
    if (status != RB_OK) {
      result = rb_ui_pop();
      rb_status_failed(status);
      return result;
    }
    return result;
  }
  /* OK stores the save, then the stock SET delegate's leaf exit pops to the
   * slot list, requests 0x930 and sets 0x009504a8. */
  if (token == rb_ui_verb_token(slot, RB_VERB_SAVE_CURRENT)) {
    uint32_t result;
    if (rb_bank_action(token, identity) != RB_OK) {
      return 1u;
    }
    result = rb_ui_stock(state, event);
    if (result == 0xffffffffu || result == 2u) {
      rb_ui_view_changed();
    }
    return result;
  }
  if (token == rb_ui_verb_token(slot, RB_VERB_RESET)) {
    uint32_t status = rb_stage_edit_reset_begin(slot, identity);
    if (status != (uint32_t)RB_OK) {
      rb_status_failed((enum rb_bank_status)status);
    }
    return 1u;
  }
  if (token == rb_ui_verb_token(slot, RB_VERB_EDIT)) {
    (void)rb_bank_action(token, identity);
    return 1u;
  }
  return rb_ui_stock(state, event);
}

/* Save Current keeps the slot's name; only the payload is replaced. */
static uint32_t rb_verb_save_current(uint8_t slot, RbMediaIdentity identity)
{
  uint32_t status = rb_save_current(slot, identity);
  if (status == (uint32_t)RB_OK) {
    rb_probe_invalidate();
  }
  return status;
}

/* The reset clears the stored name, so the resident label record follows it. */
static uint32_t rb_verb_reset(uint8_t slot, RbMediaIdentity identity)
{
  uint32_t status = rb_reset(slot, identity);
  if (status == (uint32_t)RB_OK) {
    (void)rb_slot_load(slot);
    rb_probe_invalidate();
  }
  return status;
}

static uint32_t rb_verb_run(RbBankItemRef item, RbMediaIdentity identity)
{
  switch (item.kind) {
  case RB_ITEM_VERB:
    switch ((RbBankVerb)item.verb) {
    case RB_VERB_SAVE_CURRENT:
      return rb_verb_save_current(item.slot, identity);
    case RB_VERB_EDIT_NAME:
      return rb_name_edit_open(item.slot, identity);
    case RB_VERB_EDIT:
      return rb_stage_edit_open(item.slot, identity);
    case RB_VERB_RESET:
      return rb_verb_reset(item.slot, identity);
    }
    return (uint32_t)RB_RANGE;
  case RB_ITEM_RECALL:
    return rb_recall(item.slot, identity);
  default:
    return (uint32_t)RB_RANGE;
  }
}

enum rb_bank_status rb_bank_action(uint32_t token, RbMediaIdentity identity)
{
  RbBankItemRef item = rb_ui_decode(token);
  enum rb_bank_status status;
  if (item.kind == RB_ITEM_NONE) {
    return RB_RANGE;
  }
  status = (enum rb_bank_status)rb_verb_run(item, identity);
  if (status != RB_OK) {
    rb_status_failed(status);
  } else {
    rb_ui_view_changed();
  }
  return status;
}
