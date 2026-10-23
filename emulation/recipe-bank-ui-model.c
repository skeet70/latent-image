/* The recipe-bank UI-behavior model (X-M1 1.50, host-only): the synthetic stock
 * consumer the UI cases drive the bank's menu modules through.
 *
 * The dispatch, transition and label shapes below are byte-read from the pinned
 * 1.50 firmware. Only cells registered to the interposer are modeled.
 *
 * The transition carries one frame/cursor record per depth. A push moves the
 * cursor cell to the child depth's recorded index, which is nonzero after a
 * first visit and is what rb_ui_child_enter compensates for; a pop records the
 * child's cursor and restores the parent's. A failed transition happens only
 * through the ui_refuse fixture cell.
 *
 * ui_draw_label follows the drawer's label path: a labels array quadruple field
 * carries a record id, and the record's inline UTF-16LE buffer at
 * ui_name_base + (id - 0x83c) * 0x42 is copied out.
 */
#include <stdint.h>
#include "../menu/recipe_bank_ui.h"

#define UI_FRAME_CELL 0x009571d0u
#define UI_CURSOR_CELL 0x009571d8u
#define UI_STATE_CELL 0x0146761eu
#define UI_EVENT_CELL 0x0146777au
#define UI_CELL_BASE 0x00cd1ae4u
#define UI_CELL_STRIDE 46u
#define UI_NODE_VALUES 0x20u
#define UI_NODE_LABELS 0x24u
#define UI_NODE_PARENT 0x1cu
#define UI_NODE_CLASS 0x08u
#define UI_RECORD_BASE_ID 0x83cu
#define UI_RECORD_STRIDE 0x42u
#define UI_NAME_BYTES 26u
#define UI_STACK_DEPTH 8u
#define UI_STEP_WORDS 5u
#define UI_STEP_MAX 63u

/* ui_log words */
#define UI_LOG_SET_CALLS 0u
#define UI_LOG_LEAVE_CALLS 1u
#define UI_LOG_CONFIRM_CALLS 2u
#define UI_LOG_STAGE_KIND 3u
#define UI_LOG_STAGE_ID 4u
#define UI_LOG_REDRAW 5u
#define UI_LOG_POSTLUDE 6u
#define UI_LOG_SET_CLASS 7u
#define UI_LOG_SET_TOKEN 8u
#define UI_LOG_AUX_CALLS 9u

/* step opcodes */
#define UI_OP_DISPATCH 0u
#define UI_OP_DRAW 1u
#define UI_OP_WRITE_WORD 2u
#define UI_OP_WRITE_HALF 3u
#define UI_OP_COPY 4u
#define UI_OP_CALL 5u
#define UI_OP_HALT 6u
#define UI_OP_INPUT_NAME 7u
#define UI_OP_READ_FIELD 8u
#define UI_OP_NAVIGATE 9u
#define UI_OP_REFRESH 10u
#define UI_OP_NATIVE_WRITE 11u
#define UI_EDITOR_BUFFER 0x00954385u
#define UI_EDITOR_CAPACITY 14u

/* case inputs */
volatile uint32_t ui_handler;      /* the action table word: the linked rb_bank_interpose */
volatile uint32_t ui_name_base;    /* the label record table base */
volatile uint32_t ui_epoch_bump;   /* the probe module's epoch bump entry */
volatile uint32_t ui_set_result;   /* the stock SET delegate's word */
volatile uint32_t ui_confirm_result;
volatile uint32_t ui_refuse;       /* nonzero makes the next push transition fail */

/* case outputs */
volatile uint32_t ui_out[64];
volatile uint32_t ui_log[32];
volatile uint8_t ui_out26[16][32];

/* the swap fixture's second bank file */
volatile uint8_t ui_file_b[0x420];

volatile uint32_t ui_steps[UI_STEP_MAX * UI_STEP_WORDS];

static volatile uint32_t ui_depth;
static volatile uint32_t ui_frames[UI_STACK_DEPTH];
static volatile uint32_t ui_cursors[UI_STACK_DEPTH];
static volatile uint32_t ui_leftover[UI_STACK_DEPTH];
static volatile uint32_t ui_stage_kind;

static uint32_t ui_word(uint32_t addr)
{
  return *(const volatile uint32_t *)addr;
}

static void ui_store(uint32_t addr, uint32_t value)
{
  *(volatile uint32_t *)addr = value;
}

static uint32_t ui_half(uint32_t addr)
{
  return (uint32_t)*(const volatile uint16_t *)addr;
}

static void ui_store_half(uint32_t addr, uint32_t value)
{
  *(volatile uint16_t *)addr = (uint16_t)value;
}

static void ui_copy(uint8_t *dst, const uint8_t *src, uint32_t n)
{
  uint32_t i;
  for (i = 0u; i < n; i++) {
    dst[i] = src[i];
  }
}

static uint32_t ui_call(uint32_t addr)
{
  uint32_t (*fn)(void) = (uint32_t (*)(void))(uintptr_t)addr;
  return fn();
}

static uint32_t ui_child_of_row(void)
{
  uint32_t frame = ui_word(UI_FRAME_CELL);
  uint32_t values;
  uint32_t cursor;
  if (frame == 0u) {
    return 0u;
  }
  values = ui_word(frame + UI_NODE_VALUES);
  cursor = ui_word(UI_CURSOR_CELL);
  if (values == 0u) {
    return 0u;
  }
  return ui_word(values + cursor * 8u + 4u);
}

static uint32_t ui_core_push(void)
{
  uint32_t frame;
  uint32_t cursor;
  uint32_t child;
  if (ui_refuse != 0u) {
    return 0u;
  }
  frame = ui_word(UI_FRAME_CELL);
  cursor = ui_word(UI_CURSOR_CELL);
  child = ui_child_of_row();
  if (frame == 0u || child == 0u || ui_depth + 1u >= UI_STACK_DEPTH) {
    return 0u;
  }
  ui_frames[ui_depth] = frame;
  ui_cursors[ui_depth] = cursor;
  ui_depth = ui_depth + 1u;
  ui_store(UI_FRAME_CELL, child);
  ui_store(UI_CURSOR_CELL, ui_leftover[ui_depth]);
  rb_ui_probe_visit();
  return 0xffffffffu;
}

static uint32_t ui_core_pop(void)
{
  if (ui_depth == 0u) {
    return 0u;
  }
  ui_leftover[ui_depth] = ui_word(UI_CURSOR_CELL);
  ui_depth = ui_depth - 1u;
  ui_store(UI_FRAME_CELL, ui_frames[ui_depth]);
  ui_store(UI_CURSOR_CELL, ui_cursors[ui_depth]);
  rb_ui_probe_visit();
  return 0xffffffffu;
}


static uint32_t ui_dispatch(void)
{
  uint32_t state = ui_half(UI_STATE_CELL);
  uint32_t event = ui_half(UI_EVENT_CELL);
  uint32_t cell = ui_word(UI_CELL_BASE + 4u * (state + UI_CELL_STRIDE * event));
  uint32_t action = cell & 0xffffu;
  if (action == 1u) {
    return ui_call(ui_handler);
  }
  __builtin_trap();
}

static void ui_draw_label(uint32_t page, uint32_t row, uint32_t field,
                          uint32_t out)
{
  uint32_t labels = ui_word(page + UI_NODE_LABELS);
  uint32_t id = ui_word(labels + row * 16u + field * 4u);
  uint32_t record = ui_name_base + (id - UI_RECORD_BASE_ID) * UI_RECORD_STRIDE;
  ui_copy((uint8_t *)(uintptr_t)ui_out26[out],
          (const uint8_t *)(uintptr_t)record, UI_NAME_BYTES);
}

/* ------------------------------------------------------------ the delegates */

uint32_t ui_transition(void)
{
  return (ui_stage_kind == 0x0eu) ? ui_core_push() : ui_core_pop();
}

uint32_t ui_set_dispatch(void)
{
  uint32_t frame = ui_word(UI_FRAME_CELL);
  ui_log[UI_LOG_SET_CALLS] = ui_log[UI_LOG_SET_CALLS] + 1u;
  if (frame != 0u) {
    uint32_t values = ui_word(frame + UI_NODE_VALUES);
    uint32_t cursor = ui_word(UI_CURSOR_CELL);
    ui_log[UI_LOG_SET_CLASS] = ui_word(frame + UI_NODE_CLASS);
    ui_log[UI_LOG_SET_TOKEN] = (values != 0u) ? ui_word(values + cursor * 8u) : 0u;
  }
  return ui_set_result;
}

uint32_t ui_editor_confirm(void)
{
  ui_log[UI_LOG_CONFIRM_CALLS] = ui_log[UI_LOG_CONFIRM_CALLS] + 1u;
  return ui_confirm_result;
}

uint32_t ui_leave_edge(void)
{
  ui_log[UI_LOG_LEAVE_CALLS] = ui_log[UI_LOG_LEAVE_CALLS] + 1u;
  return ui_core_pop();
}

void ui_stage_open(uint32_t kind, uint32_t id)
{
  ui_stage_kind = kind;
  ui_log[UI_LOG_STAGE_KIND] = kind;
  ui_log[UI_LOG_STAGE_ID] = id;
}

void ui_redraw(uint32_t region)
{
  ui_log[UI_LOG_REDRAW] = region;
}

void ui_postlude(uint32_t code)
{
  ui_log[UI_LOG_POSTLUDE] = code;
}

/* ------------------------------------------------------------- the case run */

uint32_t ui_run(void)
{
  uint32_t i;
  for (i = 0u; i < UI_STEP_MAX; i++) {
    const volatile uint32_t *s = &ui_steps[i * UI_STEP_WORDS];
    uint32_t op = s[0];
    if (op == UI_OP_DISPATCH) {
      uint32_t previous = ui_word(UI_FRAME_CELL);
      ui_store(UI_FRAME_CELL, s[1]);
      ui_store(UI_CURSOR_CELL, s[2]);
      ui_store_half(UI_EVENT_CELL, s[3]);
      if (previous != s[1]) {
        rb_ui_probe_visit();
      }
      ui_out[s[4]] = ui_dispatch();
    } else if (op == UI_OP_DRAW) {
      ui_draw_label(s[1], s[2], s[3], s[4]);
    } else if (op == UI_OP_WRITE_WORD) {
      ui_store(s[1], s[2]);
    } else if (op == UI_OP_WRITE_HALF) {
      ui_store_half(s[1], s[2]);
    } else if (op == UI_OP_COPY) {
      ui_copy((uint8_t *)(uintptr_t)s[1], (const uint8_t *)(uintptr_t)s[2], s[3]);
    } else if (op == UI_OP_INPUT_NAME) {
      volatile uint8_t *buffer = (volatile uint8_t *)UI_EDITOR_BUFFER;
      uint32_t j;
      for (j = 0u; j < UI_EDITOR_CAPACITY; j++) {
        buffer[j] = j < 12u ? (uint8_t)(s[1u + j / 4u] >> (8u * (j % 4u))) : 0u;
      }
    } else if (op == UI_OP_READ_FIELD) {
      uint32_t (*getter)(uint32_t, uint32_t) =
          (uint32_t (*)(uint32_t, uint32_t))(uintptr_t)s[1];
      ui_out[s[4]] = getter(s[2], 0u);
    } else if (op == UI_OP_NATIVE_WRITE) {
      uint32_t (*setter)(uint32_t, uint32_t, uint32_t) =
          (uint32_t (*)(uint32_t, uint32_t, uint32_t))(uintptr_t)s[1];
      ui_out[s[4]] = setter(s[2], s[3], 0u);
    } else if (op == UI_OP_NAVIGATE) {
      ui_store(UI_CURSOR_CELL, s[2]);
      ui_store_half(UI_EVENT_CELL, s[3]);
      ui_out[s[4]] = ui_dispatch();
    } else if (op == UI_OP_CALL) {
      ui_out[s[4]] = ui_call(s[1]);
    } else if (op == UI_OP_REFRESH) {
      rb_ui_probe_visit();
    } else if (op == UI_OP_HALT) {
      return 0u;
    } else {
      __builtin_trap();
    }
  }
  __builtin_trap();
}
