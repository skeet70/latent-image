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
#define RB_UI_CLASS_RECALL 0x285u

/* Rendering-root ids. The label resolver FUN_00bf93e0 takes a node id, looks it
 * up in the registry table 0x0081babc and resolves only that node or its
 * immediate children, so a page resolves when registry[id] is the page or its
 * immediate parent. The slot and recall pages are immediate children of the
 * shooting root (registry[1] = 0x78ea98) and keep id 1; the seven submenu pages
 * take RB_UI_SUB_ID with registry[0x61] = the slot page; the editor node takes
 * RB_UI_EDITOR_ID with registry[0x6a] = the editor node itself. Both ids are
 * zero in the registry and unused as node ids in the pinned decode. */
#define RB_UI_SUB_ID 0x61u
#define RB_UI_EDITOR_ID 0x6au

/* The stock text engine FUN_00d4ad38 serves classes 0x131, 0xb6 and 0x169 to
 * 0x16b only, and writes the wireless name for node ids 0x4d/0x56 only, so the
 * bank's editor node carries class 0x131 and its own id: typing routes through
 * the stock engine and its own confirm writes nothing, leaving the bank's write
 * to rb_name_edit_commit. The confirm row is charset token 0x2a. */
#define RB_UI_EDITOR_CLASS 0x131u
#define RB_UI_EDITOR_CONFIRM_ROW 0x2au

/* Staged-edit pages this module claims: the edit screen (child rows that push
 * the field lists and the dialog) and the dialog (action rows OK/CANCEL). The
 * nine field hosts 0x267..0x26f are value lists and stay unclaimed. */
#define RB_UI_CLASS_EDIT 0x277u
#define RB_UI_CLASS_DIALOG 0x27du
#define RB_UI_CLASS_FIELD0 0x267u
#define RB_UI_CLASS_FIELD_LAST 0x26fu
/* Edit screen row tokens: field row k (k = 0..8) carries
 * RB_UI_TOKEN_EDIT_ROW_BASE + k; the hidden row before the dialog carries
 * RB_UI_TOKEN_EDIT_HIDDEN_ROW (row index 9, token 0x82 per the emission). */
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
 * NAME row (verb 1), whose child is the editor node; recall rows carry
 * RB_UI_TOKEN_RECALL_BASE + slot; slot rows of the edit/save page are child rows
 * carrying RB_UI_TOKEN_SLOT_BASE + slot, which rb_ui_decode does not claim. */
#define RB_UI_TOKEN_VERB_BASE 0x40u
#define RB_UI_TOKEN_RECALL_BASE 0x60u
#define RB_UI_TOKEN_SLOT_BASE 0x70u

/* The two items appended to the shooting-menu page 0x0078ea98 carry these
 * tokens. 0x7c is excluded although it is the next free number: both state-0
 * dispatchers branch on class 5 with cursor 0x7c, so a token that value would
 * take a stock branch. 0x7d and 0x7e appear in no branch of 0xc72e10/0xc70528. */
#define RB_UI_TOKEN_ITEM_EDIT_SAVE 0x7du
#define RB_UI_TOKEN_ITEM_RECALL 0x7eu

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

typedef enum {
  RB_VERB_SAVE_CURRENT = 0,
  RB_VERB_EDIT_NAME = 1,
  RB_VERB_EDIT = 2,
  RB_VERB_RESET = 3
} RbBankVerb;

typedef enum {
  RB_ITEM_NONE = 0,
  RB_ITEM_VERB = 1,
  RB_ITEM_RECALL = 2
} RbBankItemKind;

typedef struct {
  RbBankItemKind kind;
  uint8_t slot;
  uint8_t verb; /* RbBankVerb when kind is RB_ITEM_VERB */
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
  /* A recalled field did not take its stored code: the stock write path left
   * the property at another value. The bank file is unchanged. */
  RB_APPLY_REFUSED = 14
};

enum rb_bank_op {
  RB_BANK_RECALL = 0,
  RB_BANK_SAVE = 1
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

static inline uint32_t rb_ui_recall_token(uint32_t slot)
{
  return RB_UI_TOKEN_RECALL_BASE + slot;
}

typedef enum {
  RB_PAGE_NONE = 0,
  RB_PAGE_SLOT,
  RB_PAGE_SUB,
  RB_PAGE_RECALL,
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
  if (cls == RB_UI_CLASS_RECALL) {
    return RB_PAGE_RECALL;
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
  RbBankItemRef item = { RB_ITEM_NONE, 0u, 0u };
  uint32_t u;
  if (token >= RB_UI_TOKEN_VERB_BASE &&
      token < RB_UI_TOKEN_VERB_BASE + RB_BANK_SLOTS * RB_UI_VERBS_PER_SLOT) {
    u = token - RB_UI_TOKEN_VERB_BASE;
    item.kind = RB_ITEM_VERB;
    item.slot = (uint8_t)(u >> 2);
    item.verb = (uint8_t)(u & 3u);
    return item;
  }
  if (token >= RB_UI_TOKEN_RECALL_BASE &&
      token < RB_UI_TOKEN_RECALL_BASE + RB_BANK_SLOTS) {
    item.kind = RB_ITEM_RECALL;
    item.slot = (uint8_t)(token - RB_UI_TOKEN_RECALL_BASE);
    return item;
  }
  return item;
}

/* Storage kernel (menu/recipe_bank_storage.c). */
extern uint32_t rb_readback(uint32_t slot, uint8_t *name_out, uint8_t *payload_out);
extern uint32_t rb_save_current(uint32_t slot, const uint8_t *name26);
extern uint32_t rb_reset(uint32_t slot);
extern uint32_t rb_recall(uint32_t slot);
extern uint32_t rb_stage_load(uint32_t slot);
extern uint32_t rb_name_commit(uint32_t slot, const uint8_t *name26);
/* Resident-only name read: RB_OK with the resident name, which is zeros for an
 * unset slot (its availability is rb_meta_op_state's answer); RB_UNSET with the
 * zero name only when no bank image is resident; RB_RANGE for a bad slot. */
extern uint32_t rb_name_resident(uint32_t slot, uint8_t *name26);
extern uint32_t rb_stage_field(uint32_t prop, uint32_t code);
extern uint32_t rb_stage_read(uint32_t prop);
extern void rb_stage_name_set(const uint8_t *name12);
extern uint32_t rb_stage_commit(uint32_t slot, const uint8_t *name26);
extern void rb_stage_cancel(void);
extern void rb_status_failed(enum rb_bank_status status);
extern void rb_status_record(enum rb_bank_status status);
extern uint32_t rb_status_last(void);

/* Name-editor glue (menu/recipe_bank_hook.c): load the slot's name into the
 * stock editor and set modal-75 state 0x1e; rb_name_edit_commit is the bank's
 * confirm write; rb_slot_load refreshes a slot's name record from the bank. */
extern uint32_t rb_name_edit_open(uint32_t slot);
extern uint32_t rb_name_edit_commit(void);
extern uint32_t rb_slot_load(uint32_t slot);

/* What a staged-edit confirmation dialog's OK runs: RB_CONFIRM_COMMIT is the
 * staged session's commit for the edit screen's leaving edge, RB_CONFIRM_ITEM is
 * one field item's leaving edge (its cancel restores the item's saved code), and
 * the Save Current and Reset items carry the other two. menu/recipe_bank_edit.h
 * includes this header instead of redefining the enum. */
enum rb_edit_confirm {
  RB_CONFIRM_NONE = 0,
  RB_CONFIRM_COMMIT = 1,
  RB_CONFIRM_SAVE = 2,
  RB_CONFIRM_RESET = 3,
  RB_CONFIRM_ITEM = 4
};

/* Staged-edit surface (menu/recipe_bank_edit.c): the edit screen's entry, its
 * leaving edge, the field-row select and item leaving edge, and the dialog
 * entries. */
extern uint32_t rb_stage_edit_open(uint32_t slot);
extern uint32_t rb_stage_edit_leave(void);
extern uint32_t rb_stage_edit_row(uint32_t host, uint32_t row);
extern uint32_t rb_stage_edit_item_leave(void);
extern uint32_t rb_stage_edit_confirm_begin(enum rb_edit_confirm what, uint32_t slot);
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
extern void rb_call_postlude(uint32_t code);                /* FUN_00d38f34 */

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

/* Storage lane's read-only verdict for one bank operation. */
uint32_t rb_meta_op_state(enum rb_bank_op op, uint32_t slot, uint32_t reload);

/* The bank's menu entries. rb_bank_interpose is the word modal-75 action-table
 * index 1 holds; it returns the transition result for entered rows and the stock
 * handler's result otherwise. The two rows appended to the shooting root are
 * claimed by their tokens and their children's page classes and pushed through
 * rb_ui_child_enter; every other root row delegates. rb_bank_action decodes one
 * bank token, returning 1 when it handled the token and 0 when the token is not
 * a bank item. rb_ui_child_enter pushes the selected row's child with the
 * semantics this module owns (parent repointing with refusal rollback, cursor
 * reset, name republication) and is the entry the staged-edit module calls for
 * its dialog and edit-screen pushes. */
uint32_t rb_bank_interpose(void);
uint32_t rb_bank_action(uint32_t token);
uint32_t rb_ui_child_enter(void);

#endif
