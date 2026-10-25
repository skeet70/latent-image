/* The editable recipe bank's menu contract (X-M1 1.50, host-only, candidate).
 * menu/recipe_bank_actions.c implements the actions.
 *
 * Stock facts this header depends on (byte-read from the pinned 1.50 decodes):
 *
 * - The current menu frame is the current node header itself: FUN_00bf976c
 *   stores the node pointer in 0x009571d0. Node +0x08 is its X word (the page
 *   class the handlers compare), node +0x20 is its values array, and
 *   0x009571d8 holds the selected row index, so the selected row's first word
 *   is *(values + cursor*8) (FUN_0061cd14 word 4 = FUN_0061d244).
 * - Modal 75 (0x4b) is the settings-menu modal. Its action table is the
 *   299-word array at 0xd04080 (pointer slot 0xcfe3d4), its width is 46, and
 *   its state-0 SET cell is 0xcd20a4 = {action 0x5a, next 1}, where action 0x5a
 *   is the per-item SET dispatcher FUN_00c72e10; the editor's confirm cell
 *   (state 0x1e, event 8) is 0xcd211c = {action 115, next 1}, where action 115
 *   is FUN_00c75ed4. Table words 0 and 1 are zero and no cell references index
 *   1, so index 1 is the interposition slot: both cells become index 1 and word
 *   1 becomes rb_bank_interpose, which delegates to the stock handler of the
 *   cell's state for every page and token it does not claim.
 * - The walker passes the node X to the row resolver (FUN_00bfa800 ->
 *   FUN_00d588b8(prop)), so each page class keeps a valid property number whose
 *   conversion slot (0x81b09c + 4*prop) is null and whose commit word is the
 *   shared no-op arm 0xd52664.
 */
#ifndef RECIPE_BANK_UI_H
#define RECIPE_BANK_UI_H

#include <stdint.h>
#include <stdbool.h>

#define RB_UI_VERBS_PER_SLOT 4u

/* One X per page. The stock resolvers match a page by its X within the shooting
 * root's subtree and take the first match (FUN_00bf8f4c, FUN_00bf90f0,
 * FUN_00bf93e0), so the nine pages cannot share one value. Every value below
 * sits outside the render probe's 5..0x260 jump table, has a null conversion
 * slot (0x81b09c + 4*class) and the shared no-op commit word 0xd52664, and no
 * dumped node carries it as X. */
#define RB_UI_CLASS_SLOT 0x281u
#define RB_UI_CLASS_SUB0 0x270u
#define RB_UI_CLASS_SUB(slot) (RB_UI_CLASS_SUB0 + (slot))
#define RB_UI_CLASS_SELECT 0x285u

/* FUN_00bf93e0 resolves the registered root and its immediate children.
 * Slot and its verb pages share root 0x61; SELECT CUSTOM SETTING uses shooting
 * root 1.
 * Edit and its fields share root 0x72; the dialog uses root 0x29. */
#define RB_UI_SUB_ID 0x61u
#define RB_UI_EDITOR_ID 0x6au

/* The stock text engine FUN_00d4ad38 serves classes 0x131, 0xb6 and 0x169 to
 * 0x16b only, and writes the wireless name for node ids 0x4d/0x56 only, so the
 * bank's editor node carries class 0x131 and its own id: typing routes through
 * the stock engine and its own confirm writes nothing, leaving the bank's write
 * to rb_name_edit_commit. Rows 0x27..0x29 move the cursor left and right and
 * delete, row 0x2a confirms, and every row below 0x27 inserts a character. */
#define RB_UI_EDITOR_CLASS 0x131u
#define RB_UI_EDITOR_MOVE_LEFT_ROW 0x27u
#define RB_UI_EDITOR_CONFIRM_ROW 0x2au

/* Edit pages this module claims: the edit screen (child rows that push the
 * field lists) and the dialog (action rows OK/CANCEL). The nine field hosts
 * 0x267..0x26f are value lists. */
#define RB_UI_CLASS_EDIT 0x277u
#define RB_UI_CLASS_DIALOG 0x27du
#define RB_UI_CLASS_FIELD0 0x267u
#define RB_UI_CLASS_FIELD_LAST 0x26fu
/* Edit screen field row k (k = 0..8) carries RB_UI_TOKEN_EDIT_ROW_BASE + k.
 * The verb pages' hidden dialog row carries RB_UI_TOKEN_EDIT_HIDDEN_ROW. */
#define RB_UI_TOKEN_EDIT_ROW_BASE 0x70u
#define RB_UI_TOKEN_EDIT_HIDDEN_ROW 0x82u
#define RB_UI_TOKEN_DIALOG_OK 0x80u
#define RB_UI_TOKEN_DIALOG_CANCEL 0x81u

/* Modal-75 key event cell (halfword) and the two events this module acts on. */
#define RB_UI_EVENT_CELL 0x0146777au
#define RB_UI_EVENT_SET 0x8u
#define RB_UI_EVENT_LEAVE 0xdu

/* Modal-75 state halfword; 0x1e is the text editor state the stock confirm
 * action FUN_00c75ed4 (action 115, cell (30,8)) serves. */
#define RB_UI_STATE_CELL 0x146761eu
#define RB_UI_STATE_BROWSE 0x0u
#define RB_UI_STATE_EDITOR 0x1eu

/* Row tokens in the bank pages' values arrays. Verb rows carry
 * RB_UI_TOKEN_VERB_BASE + slot*4 + verb and child 0, except the EDIT CUSTOM
 * NAME row (verb 1), whose child is the editor node; SELECT CUSTOM SETTING's
 * rows carry RB_UI_TOKEN_SELECT_BASE + the selection code, row index and code
 * alike; slot rows of the edit/save page are child rows carrying
 * RB_UI_TOKEN_SLOT_BASE + slot, which rb_ui_decode does not claim. */
#define RB_UI_TOKEN_VERB_BASE 0x40u
#define RB_UI_TOKEN_SELECT_BASE 0x60u
#define RB_UI_SELECT_ROWS 8u
#define RB_UI_TOKEN_SLOT_BASE 0x70u

/* The two items appended to the shooting-menu page 0x0078ea98 carry these
 * tokens. 0x7c is excluded although it is the next free number: both state-0
 * dispatchers branch on class 5 with cursor 0x7c, so a token that value would
 * take a stock branch. 0x7d and 0x7e appear in no branch of 0xc72e10/0xc70528. */
#define RB_UI_TOKEN_ITEM_EDIT_SAVE 0x7du
#define RB_UI_TOKEN_ITEM_SELECT 0x7eu

/* Row-count bound the stock walker itself uses (0x4f rows per page). */
#define RB_UI_ROW_CAP 0x50u

/* Node header layout (0x2c bytes). */
#define RB_NODE_BYTES 0x2cu
#define RB_NODE_ID 0x00u
#define RB_NODE_TYPE 0x04u
#define RB_NODE_X 0x08u
#define RB_NODE_KIND 0x0cu
#define RB_NODE_FLAG 0x18u
#define RB_NODE_PARENT 0x1cu
#define RB_NODE_VALUES 0x20u
#define RB_NODE_LABELS 0x24u
#define RB_NODE_SELECTOR 0x28u

/* Stock cells the interposer reads. */
#define RB_UI_FRAME_CELL 0x009571d0u
#define RB_UI_CURSOR_CELL 0x009571d8u
#define RB_UI_VISIBLE_CURSOR_CELL 0x009571dcu
/* FUN_01165aa0 draws an open list's current-value marker on the row whose token
 * is values[this row], read through a fresh FUN_0061cd14 snapshot (word 5).
 * Only FUN_00bfa800's tail (a copy of the cursor cell, skipped for walk kind
 * 0x13) and FUN_0061d3a8's menu-block restore write it. */
#define RB_UI_CURRENT_ROW_CELL 0x0095732cu

typedef struct {
  uint32_t address;
} RbMenuFrame;

#ifdef RB_RAM_DIAGNOSTICS
typedef struct {
  uint32_t sequence;
} RbDiagnosticWalk;
#endif

typedef struct {
  uint32_t value;
} RbMenuRoot;

typedef struct {
  uint32_t value;
} RbViewRevision;

typedef struct {
  uint32_t id;
} RbMessageLabel;

typedef struct {
  RbMessageLabel text;
  RbMessageLabel prefix;
  RbMessageLabel button;
} RbMessageLabels;

extern volatile RbViewRevision rb_view_revision;

static inline __attribute__((always_inline)) void rb_ui_view_changed(void)
{
  rb_view_revision.value += 1u;
}

typedef enum {
  RB_VERB_SAVE_CURRENT = 0,
  RB_VERB_EDIT_NAME = 1,
  RB_VERB_EDIT = 2,
  RB_VERB_RESET = 3
} RbBankVerb;

typedef enum {
  RB_ITEM_NONE = 0,
  RB_ITEM_VERB = 1,
  RB_ITEM_SELECT = 2
} RbBankItemKind;

/* A persisted bank selection: code 0 is NOTHING SELECTED and code n (1-7) is
 * CUSTOM n, the v2 file's selection byte. */
typedef struct {
  uint8_t code;
} RbSelection;

#define RB_SELECTION_NONE ((RbSelection){0u})

typedef struct {
  RbBankItemKind kind;
  uint8_t slot;
  uint8_t verb; /* RbBankVerb when kind is RB_ITEM_VERB */
  RbSelection selection; /* when kind is RB_ITEM_SELECT */
} RbBankItemRef;

/* The caller-visible status domain of menu/recipe_bank_storage.c's kernel. */
enum rb_bank_status {
  RB_OK = 0,
  RB_NOT_READY = 1,
  RB_READ_ONLY = 2,
  RB_FULL = 3,
  RB_IO = 4,
  RB_INVALID = 5,
  RB_UNSET = 6,
  RB_RANGE = 7,
  RB_AT_C = 8,
  RB_NO_BANK = 9,
  RB_GEN_EXHAUST = 10,
  RB_TASK_REFUSED = 11,
  RB_MISSING = 12,
  /* The media epoch or identity changed between the operation's start snapshot
   * and its publish or commit point: nothing was published or written, and the
   * caller must not retry within the visit. */
  RB_MEDIA_CHANGED = 13,
  /* A bank field did not take its stored code, or a selection's base or
   * target was refused: the stock write path left a property at another
   * value or the mode is outside P, S, A and M. */
  RB_APPLY_REFUSED = 14,
  /* The newest checksum-valid generation holds a payload no stock menu
   * produces: nothing is published and no write replaces either file. */
  RB_UNSAFE_BANK = 15,
  /* The shooting mode moved during a selection's I/O: nothing was applied. */
  RB_MODE_CHANGED = 16,
  /* The slot is the persisted selection, which a reset does not clear. */
  RB_SLOT_SELECTED = 17
};

/* FUN_00d52684's row states, which the list machinery consumes. */
enum rb_row_state {
  RB_ROW_ACTIVE = 0,
  RB_ROW_DISABLED = 2,
  RB_ROW_HIDDEN = 3
};

#define RB_BANK_SLOTS 7u

static inline uint32_t rb_ui_verb_token(uint32_t slot, RbBankVerb verb)
{
  return RB_UI_TOKEN_VERB_BASE + slot * RB_UI_VERBS_PER_SLOT + (uint32_t)verb;
}

static inline uint32_t rb_ui_select_token(RbSelection selection)
{
  return RB_UI_TOKEN_SELECT_BASE + selection.code;
}

typedef enum {
  RB_PAGE_NONE = 0,
  RB_PAGE_SLOT,
  RB_PAGE_SUB,
  RB_PAGE_SELECT,
  RB_PAGE_EDITOR,
  RB_PAGE_EDIT,
  RB_PAGE_DIALOG,
  RB_PAGE_FIELD
} RbBankPage;

/* Page kind of a frame class; RB_PAGE_SUB also yields the slot. */
static inline RbBankPage rb_ui_page(uint32_t cls, uint8_t *slot_out)
{
  if (cls == RB_UI_CLASS_SLOT) {
    return RB_PAGE_SLOT;
  }
  if (cls >= RB_UI_CLASS_SUB0 && cls < RB_UI_CLASS_SUB0 + RB_BANK_SLOTS) {
    if (slot_out != 0) {
      *slot_out = (uint8_t)(cls - RB_UI_CLASS_SUB0);
    }
    return RB_PAGE_SUB;
  }
  if (cls == RB_UI_CLASS_SELECT) {
    return RB_PAGE_SELECT;
  }
  if (cls == RB_UI_EDITOR_CLASS) {
    return RB_PAGE_EDITOR;
  }
  if (cls == RB_UI_CLASS_EDIT) {
    return RB_PAGE_EDIT;
  }
  if (cls == RB_UI_CLASS_DIALOG) {
    return RB_PAGE_DIALOG;
  }
  if (cls >= RB_UI_CLASS_FIELD0 && cls <= RB_UI_CLASS_FIELD_LAST) {
    return RB_PAGE_FIELD;
  }
  return RB_PAGE_NONE;
}

/* Token only: the caller has already established the page class. Ranges that
 * are not bank action tokens decode as RB_ITEM_NONE, including the slot rows of
 * the edit/save page. */
static inline RbBankItemRef rb_ui_decode(uint32_t token)
{
  RbBankItemRef item = { RB_ITEM_NONE, 0u, 0u, RB_SELECTION_NONE };
  uint32_t u;
  if (token >= RB_UI_TOKEN_VERB_BASE &&
      token < RB_UI_TOKEN_VERB_BASE + RB_BANK_SLOTS * RB_UI_VERBS_PER_SLOT) {
    u = token - RB_UI_TOKEN_VERB_BASE;
    item.kind = RB_ITEM_VERB;
    item.slot = (uint8_t)(u >> 2);
    item.verb = (uint8_t)(u & 3u);
    return item;
  }
  if (token >= RB_UI_TOKEN_SELECT_BASE &&
      token < RB_UI_TOKEN_SELECT_BASE + RB_UI_SELECT_ROWS) {
    item.kind = RB_ITEM_SELECT;
    item.selection = (RbSelection){(uint8_t)(token - RB_UI_TOKEN_SELECT_BASE)};
    return item;
  }
  return item;
}

/* Mutation identity spans discovery, staging, verification and publication. */
typedef struct {
  uint32_t epoch;
} RbMediaIdentity;

RbMediaIdentity rb_media_sample(void);
bool rb_media_matches(RbMediaIdentity identity);
typedef void (*RbIoCallback)(uint32_t, uint32_t);
RbMediaIdentity rb_io_request_identity(void);
bool rb_media_worker_allows(RbIoCallback callback, RbMediaIdentity identity);

/* Storage kernel (menu/recipe_bank_storage.c). */
extern uint32_t rb_load(void);
extern uint32_t rb_readback(uint32_t slot, uint8_t *name_out, uint8_t *payload_out,
                            RbMediaIdentity identity);
extern uint32_t rb_save_current(uint32_t slot, RbMediaIdentity identity);
extern uint32_t rb_reset(uint32_t slot, RbMediaIdentity identity);
/* The selection is written only after the base and the target are applied and
 * read back live; a failure puts back the live settings read before the
 * transaction. */
extern uint32_t rb_select(RbSelection selection, RbMediaIdentity identity);
/* The selection whose settings are live in this session, false while a
 * remembered custom selection is suspended or no bank is resident. */
extern bool rb_selection_effective(RbSelection *selection_out);
/* Dial boundaries: rb_mode_leave runs before a stock mode change to (mode,
 * class), rb_mode_resume after it; rb_mode_dial_apply and rb_mode_pair_apply
 * wrap the two stock mode-apply calls with them. */
extern uint32_t rb_mode_leave(uint32_t next_mode, uint32_t next_class);
extern uint32_t rb_mode_resume(void);
extern uint32_t rb_mode_dial_apply(void);
extern void rb_mode_pair_apply(uint32_t next_mode, uint32_t next_class);
extern uint32_t rb_stage_load(uint32_t slot, RbMediaIdentity identity);
extern uint32_t rb_name_commit(uint32_t slot, const uint8_t *name26,
                               RbMediaIdentity identity);
/* Resident-only name read: RB_OK with the resident name, which is zeros for an
 * unset slot (its availability is rb_meta_select_state's answer); RB_UNSET with the
 * zero name only when no bank image is resident; RB_RANGE for a bad slot. */
extern uint32_t rb_name_resident(uint32_t slot, uint8_t *name26);
/* The session's fields are the nine edit properties and RB_ISO_AUTO_FLAG, the
 * payload's ISO auto flag (the 0x007a0018 entry of modes 0x21-0x24), which a
 * new 0x141 code stages with it. */
#define RB_ISO_AUTO_FLAG 0x146u
extern uint32_t rb_stage_field(uint32_t prop, uint32_t code);
extern uint32_t rb_stage_read(uint32_t prop);
extern void rb_stage_name_set(const uint8_t *name12);
extern uint32_t rb_stage_commit(uint32_t slot, const uint8_t *name26);
extern void rb_stage_cancel(void);
extern void rb_status_failed(enum rb_bank_status status);
extern void rb_status_present(RbMessageLabels labels);
extern void rb_status_record(enum rb_bank_status status);
extern uint32_t rb_status_last(void);

/* Name-editor glue (menu/recipe_bank_hook.c): load the slot's name into the
 * stock editor and set modal-75 state 0x1e; rb_name_edit_commit is the bank's
 * confirm write; rb_name_edit_full reports whether the editor already holds
 * the bank's twelve characters; rb_slot_load refreshes a slot's name record
 * from the bank. */
extern uint32_t rb_name_edit_open(uint32_t slot, RbMediaIdentity identity);
extern uint32_t rb_name_edit_commit(void);
extern bool rb_name_edit_full(void);
extern uint32_t rb_slot_load(uint32_t slot);

/* Edit surface (menu/recipe_bank_edit.c): the edit screen's entry and leaving
 * edge, the field-row OK, and the dialog entries. */
extern uint32_t rb_stage_edit_open(uint32_t slot, RbMediaIdentity identity);
extern void rb_stage_edit_close(void);
extern uint32_t rb_stage_edit_row(uint32_t host, uint32_t row);
extern uint32_t rb_stage_edit_reset_begin(uint32_t slot, RbMediaIdentity identity);
extern uint32_t rb_stage_edit_dialog_ok(void);
extern uint32_t rb_stage_edit_dialog_cancel(void);

/* Stock tail the interposer delegates to for the leaving-edge cell: the
 * class-checked nav pop FUN_00c76650 (action 129, cell (0,13)). */
extern uint32_t rb_call_leave_edge(void);

/* Stock tails the interposer delegates to, by state: the modal-75 state-0 SET
 * dispatcher FUN_00c72e10 and the editor confirm action FUN_00c75ed4. */
extern uint32_t rb_call_set_dispatch(void);
extern uint32_t rb_call_editor_confirm(void);

/* Menu framework transitions, from menu/recipe_bank_arm.S trampolines. The
 * child-enter pattern is rb_call_stage_open(0xe, 0x1d) then rb_call_transition(),
 * and the confirm exit is rb_call_stage_open(0xf, 0x1d) then rb_call_transition(). */
extern void rb_call_stage_open(uint32_t kind, uint32_t id); /* FUN_00d3d858 */
extern uint32_t rb_call_transition(void);                   /* FUN_00c12aac */
extern void rb_call_snapshot_store(RbMenuRoot root);        /* FUN_0061ceac */
extern void rb_call_postlude(uint32_t code);                /* FUN_00d38f34 */
extern void rb_call_redraw(uint32_t code);
extern void rb_call_stock_present(uint32_t event);
extern void rb_call_preview_clear(void);
extern void rb_call_stock_draw_root(uint32_t root);
extern void rb_call_draw_root_prepare(uint32_t preparation);
extern void rb_call_draw_pane(uint32_t root, uint32_t kind);
extern void rb_call_draw_action_list(RbMenuRoot root);

extern uint32_t rb_call_name_style(uint32_t variant);
extern void rb_call_draw_mode(uint32_t mode);
extern void rb_call_name_background(uint32_t background);
extern void rb_call_name_title(uint32_t label, uint32_t variant);
extern void rb_call_name_rect(int32_t left, int32_t top, int32_t right,
                             int32_t bottom, uint8_t mode,
                             uint32_t material, uint32_t opacity);
extern void rb_call_name_shell(void);
extern void rb_call_name_label(int32_t x, int32_t y, uint32_t label);
extern void rb_call_name_material(uint32_t material, uint32_t opacity,
                                 uint32_t background, uint32_t flags);
extern void rb_call_name_utf16(int8_t x, int32_t y, const uint16_t *text);
extern void rb_call_name_stretch(uint32_t left, int8_t right, uint32_t y,
                                int8_t height, int8_t scale, uint32_t glyph);
extern void rb_call_name_ascii(int8_t x, int32_t y, const char *text);
extern uint32_t rb_call_name_footer(uint32_t label);
extern void rb_call_name_label_position(uint32_t label, int8_t *x, int8_t *y);
extern uint32_t rb_call_backdrop_suppressed(void);
extern uint32_t rb_call_stock_key_right(void);
extern uint32_t rb_call_stock_key_left(void);
extern uint32_t rb_call_editor_cancel(void);
extern uint32_t rb_name_edit_cancel(void);
void rb_bank_draw_root(uint32_t root);
/* Points RB_UI_CURRENT_ROW_CELL at the open field list's staged row. */
void rb_bank_field_current_row(void);
void rb_bank_draw_name(void);
uint32_t rb_bank_key_right(void);
uint32_t rb_bank_key_left(void);
void rb_ui_probe_visit(void);
enum rb_row_state rb_ui_probe_state(uint32_t property, uint32_t row);
void rb_bank_present(uint32_t event);

/* Availability lane's stock-ABI render probe: the raw property and row words
 * the blob entry passes in r5/r6, the row state out in r0. */
enum rb_row_state rb_probe_state(uint32_t property, uint32_t row);

/* Every path that changes the bank's stored state calls this: this module's
 * save and reset verbs, the name-editor confirm, the staged-edit commit, and
 * whatever detects a card change. */
void rb_probe_invalidate(void);

/* Availability lane's media epoch: bumped only by the attach and detach worker
 * entry shims (0x0071fa9c/0x0071f7dc), read by the probe cache and the storage
 * scan-coherence check. The close worker 0x0071f1d4's FUN_0071d66c call stays
 * epoch-inert. */
extern uint32_t rb_media_epoch(void);
extern void rb_media_epoch_bump(void);

/* Storage lane's read-only verdicts: whether a save of the slot, or the
 * selection, would be attempted. */
uint32_t rb_meta_save_state(uint32_t slot, uint32_t reload);
#ifdef RB_RAM_DIAGNOSTICS
uint32_t rb_meta_select_state(RbSelection selection, uint32_t reload, RbDiagnosticWalk walk);
#else
uint32_t rb_meta_select_state(RbSelection selection, uint32_t reload);
#endif

/* rb_bank_interpose is the word modal-75 action-table
 * index 1 holds; it returns the transition result for entered rows and the stock
 * handler's result otherwise. The two rows appended to the shooting root are
 * claimed by their tokens and their children's page classes and pushed through
 * rb_ui_child_enter; every other root row delegates.
 * rb_ui_child_enter pushes the selected row's child with the
 * semantics this module owns (parent repointing with refusal rollback, cursor
 * placement, name republication) and is the entry the staged-edit module calls for
 * its dialog and edit-screen pushes. */
uint32_t rb_bank_interpose(void);
/* RB_RANGE for a token outside the bank. */
enum rb_bank_status rb_bank_action(uint32_t token, RbMediaIdentity identity);
uint32_t rb_ui_child_enter(void);

#endif
