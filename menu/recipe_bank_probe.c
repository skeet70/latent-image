/* The editable recipe bank's row-availability probe and commit-failure display
 * (X-M1 1.50, candidate, non-installable).
 *
 * Interposition. The render probe FUN_00d52684 dispatches properties 5..0x260
 * through a 604-word jump table and sends every property outside it to the
 * default arm 0x00d58640, which returns 0 (byte-read at 0x00d526ec-0x00d52700:
 * movw r2,#0x25b; ldrls pc,[pc,r3,lsl#2]; b 0xd58640; 0x00d58640: mov r4,#0;
 * b 0xd5868c). The plan writes the blob's rb_probe_entry over the branch at
 * 0x00d526fc; the entry calls rb_probe_state(property, row token) and returns
 * the state through the probe's own epilogue at 0x00d5868c.
 *
 * Rows. The recall page carries RB_UI_CLASS_RECALL (0x285) as its X word and its
 * rows are the values tokens 0x60..0x66 (menu/recipe_bank_ui.h). The seven
 * submenus (0x270..0x276) hide the RB_UI_TOKEN_EDIT_HIDDEN_ROW (0x82) dialog
 * placeholder. Verb, slot, edit-screen, field-list and dialog rows stay
 * selectable; their commit-time failures surface through rb_status_failed.
 *
 * Verdict lifetime. The seven recall verdicts are valid only inside one page
 * visit. They are dropped at the real page-entry boundary (the plan points
 * FUN_00bfa800's three call sites 0x00bfa3f0, 0x00bfa7ec and 0x00bfacf0 at the
 * blob's rb_bank_page_entry, which invalidates and then calls FUN_00bfa800,
 * before the open walk probes rows), on the media epoch (the detach worker
 * FUN_0071fa9c and the attach worker FUN_0071f7dc, retargeted at
 * rb_media_epoch_entry_a/b, bump rb_media_epoch once per device event, so a
 * removal or replacement is seen on the next row probe with no page change and
 * no dependence on the status byte or the card handle value), and when the
 * store-changing paths call rb_probe_invalidate (menu/recipe_bank_actions.c
 * Save Current and Reset, the name-editor confirm, the field-value OK). One
 * reload attempt per trigger; the storage lane refuses a scan whose media epoch
 * moved under it, and the next row probe re-arms when the epoch moved during
 * the attempt.
 *
 * State domain. FUN_00bf8d20 stores the probe's result at row+0x10, default 3;
 * FUN_00bfa800 stores one state per entry at 0x009571f0; FUN_00bf9b1c stops the
 * cursor only on entries whose state is below 2; FUN_00bf9794 mode 0x36 and the
 * draw loop in FUN_01164e1c exclude state 3 from the displayed count;
 * FUN_01163e04 draws a state-2 row with material 0x6b against 0x6a for the
 * ordinary selected row.
 */
#include "recipe_bank_ui.h"

/* the dial position reads 3 at C (recipe-design section 16; storage lane's
 * RB_MODE_CLASS) */
#define RB_MODE_CLASS 0x00951564u

#define RB_MSG_GATE 0x01139e94u
#define RB_MSG_EMIT 0x011392e0u

typedef enum {
    RB_MSG_NO_CARD = 0x7e1u,
    RB_MSG_CARD_ERROR = 0x7e2u,
    RB_MSG_PROTECTED = 0x7e3u,
    RB_MSG_MEMORY_FULL = 0x7e5u,
    RB_MSG_WRITE_ERROR = 0x7e7u,
    RB_MSG_CANNOT_EXEC = 0x807u
} RbFailureLabel;

typedef enum {
    RB_REDRAW_FAILURE_MENU = 0x400u,
    RB_REDRAW_FAILURE_EDITOR = 0x148u
} RbFailureRedraw;

static uint32_t rb_media_epoch_value;
static uint32_t rb_probe_epoch = 0xffffffffu;
static uint32_t rb_probe_dirty = 1u;
static uint32_t rb_probe_recall[RB_BANK_SLOTS];

typedef struct {
    uint32_t address;
    RbViewRevision visit;
} RbFailureFrame;

typedef struct {
    uint32_t value;
} RbFailureRevision;

typedef struct {
    uint32_t value;
} RbRedrawRevision;

typedef enum {
    RB_FAILURE_DRAW_OTHER,
    RB_FAILURE_DRAW_MENU,
    RB_FAILURE_DRAW_EDITOR
} RbFailureDrawer;

volatile RbViewRevision rb_view_revision;

/* MainUI publishes errors for its view; graphics redraws them until the view changes. */
static volatile struct {
    RbFailureFrame frame;
    enum rb_bank_status status;
    RbFailureDrawer drawer;
} rb_failure;
static volatile RbFailureRevision rb_failure_revision;

/* Graphics publishes redraw requests; MainUI owns their acknowledgement. */
static volatile RbFailureRevision rb_redraw_failure;
static volatile RbRedrawRevision rb_redraw_revision;
static RbRedrawRevision rb_redraw_consumed;

extern void rb_call_stock_redraw_dispatch(void);


static uint32_t rb_probe_word(uint32_t addr)
{
    return *(const volatile uint32_t *)addr;
}

/* The media transition witness. FUN_0071fa9c is the detach worker (clears the
 * card handle at 0x00910204 + 0x114 and the context) and FUN_0071f7dc the
 * attach worker (establishes both for the enumerated device); each runs once per
 * device event, and neither is reached from the file-request path, whose close
 * worker calls FUN_0071d66c and must not move the epoch. Their entries,
 * retargeted at rb_media_epoch_entry_a/b, bump on every attach and detach even
 * when the handle value ends unchanged. */
uint32_t rb_media_epoch(void) { return rb_media_epoch_value; }

/* Called from the rb_media_epoch_entry_a/b shims before the stock prologue
 * replays. */
void rb_media_epoch_bump(void) { rb_media_epoch_value = rb_media_epoch_value + 1u; }

static void rb_probe_reload(void)
{
    uint32_t slot;
    for (slot = 0u; slot < RB_BANK_SLOTS; slot++) {
        rb_probe_recall[slot] =
            rb_meta_op_state(RB_BANK_RECALL, slot, (slot == 0u) ? 1u : 0u);
    }
}

/* Called at page entry by rb_bank_page_entry and after every bank write by the
 * store-changing paths. */
void rb_probe_invalidate(void) { rb_probe_dirty = 1u; }

/* Pages whose values arrays carry the hidden dialog placeholder row: the seven
 * slot submenus. */
static uint32_t rb_probe_hidden_page(uint32_t property)
{
    return (property >= RB_UI_CLASS_SUB0 &&
            property < RB_UI_CLASS_SUB0 + RB_BANK_SLOTS)
               ? 1u
               : 0u;
}

enum rb_row_state rb_probe_state(uint32_t property, uint32_t row)
{
    uint32_t epoch = rb_media_epoch_value;
    uint32_t slot;
    if (epoch != rb_probe_epoch) {
        rb_probe_epoch = epoch;
        rb_probe_dirty = 1u;
    }
    if (rb_probe_hidden_page(property) != 0u) {
        return (row == RB_UI_TOKEN_EDIT_HIDDEN_ROW) ? RB_ROW_HIDDEN
                                                    : RB_ROW_ACTIVE;
    }
    if (property != RB_UI_CLASS_RECALL) {
        return RB_ROW_ACTIVE;
    }
    if (row < RB_UI_TOKEN_RECALL_BASE ||
        row >= RB_UI_TOKEN_RECALL_BASE + RB_BANK_SLOTS) {
        return RB_ROW_ACTIVE;
    }
    if (rb_probe_dirty != 0u) {
        /* one attempt per trigger; the epoch of this attempt is kept, so a
         * transition during the load re-arms on the next row probe and the
         * storage lane's before/after check refuses a mixed scan */
        rb_probe_epoch = epoch;
        rb_probe_reload();
        rb_probe_dirty = 0u;
    }
    slot = row - RB_UI_TOKEN_RECALL_BASE;
    if (rb_probe_word(RB_MODE_CLASS) == 3u) {
        return RB_ROW_DISABLED;
    }
    return (rb_probe_recall[slot] == RB_OK) ? RB_ROW_ACTIVE : RB_ROW_DISABLED;
}

static RbFailureLabel rb_status_label(enum rb_bank_status status)
{
    switch (status) {
    case RB_NOT_READY:
        return RB_MSG_NO_CARD;
    case RB_READ_ONLY:
        return RB_MSG_PROTECTED;
    case RB_FULL:
        return RB_MSG_MEMORY_FULL;
    case RB_IO:
    case RB_MISSING:
    case RB_MEDIA_CHANGED:
        return RB_MSG_CARD_ERROR;
    case RB_GEN_EXHAUST:
        return RB_MSG_WRITE_ERROR;
    default:
        return RB_MSG_CANNOT_EXEC;
    }
}

void rb_status_failed(enum rb_bank_status status)
{
    uint8_t slot;
    RbFailureFrame frame = { rb_probe_word(RB_UI_FRAME_CELL), rb_view_revision };
    RbBankPage page = frame.address == 0u ? RB_PAGE_NONE
        : rb_ui_page(rb_probe_word(frame.address + RB_NODE_X), &slot);
    rb_status_record(status);
    RbFailureRevision revision = rb_failure_revision;
    rb_failure_revision.value = revision.value + 1u;
    rb_failure.status = status;
    rb_failure.frame = frame;
    rb_failure.drawer = page == RB_PAGE_EDITOR
        ? RB_FAILURE_DRAW_EDITOR : RB_FAILURE_DRAW_MENU;
    rb_failure_revision.value = revision.value + 2u;
    rb_call_redraw(page == RB_PAGE_EDITOR
        ? RB_REDRAW_FAILURE_EDITOR : RB_REDRAW_FAILURE_MENU);
}

/* Called after the selected drawer; halt and no-draw paths keep the stock join. */
void rb_bank_emit_failure_from_stock(uint32_t packet, RbViewRevision drawn)
{
    RbFailureDrawer drawer = packet == 0xf1u ? RB_FAILURE_DRAW_MENU
        : packet == 0x47u ? RB_FAILURE_DRAW_EDITOR : RB_FAILURE_DRAW_OTHER;
    if (drawer == RB_FAILURE_DRAW_OTHER) {
        return;
    }
    RbFailureRevision revision = rb_failure_revision;
    if ((revision.value & 1u) != 0u) {
        return;
    }
    RbFailureFrame frame = rb_failure.frame;
    enum rb_bank_status status = rb_failure.status;
    RbFailureDrawer expected = rb_failure.drawer;
    if (rb_failure_revision.value != revision.value) {
        return;
    }
    if (drawn.value != frame.visit.value) {
        return;
    }
    if (frame.address == 0u ||
        frame.visit.value != rb_view_revision.value ||
        frame.address != rb_probe_word(RB_UI_FRAME_CELL)) {
        return;
    }
    if (drawer != expected) {
        return;
    }
    bool (*allows)(void) = (bool (*)(void))RB_MSG_GATE;
    bool allowed = allows();
    if (frame.visit.value != rb_view_revision.value ||
        frame.address != rb_probe_word(RB_UI_FRAME_CELL) ||
        rb_failure_revision.value != revision.value) {
        return;
    }
    if (!allowed) {
        RbRedrawRevision request = rb_redraw_revision;
        rb_redraw_revision.value = request.value + 1u;
        rb_redraw_failure = revision;
        rb_redraw_revision.value = request.value + 2u;
        return;
    }
    void (*emit)(RbFailureLabel, uint32_t, uint32_t) =
        (void (*)(RbFailureLabel, uint32_t, uint32_t))RB_MSG_EMIT;
    emit(rb_status_label(status), 0u, 0u);
}

void rb_bank_redraw_dispatch(void)
{
    rb_call_stock_redraw_dispatch();
    RbRedrawRevision request = rb_redraw_revision;
    if ((request.value & 1u) != 0u ||
        request.value == rb_redraw_consumed.value) {
        return;
    }
    RbFailureRevision wanted = rb_redraw_failure;
    if (rb_redraw_revision.value != request.value) {
        return;
    }
    RbFailureRevision revision = rb_failure_revision;
    if ((revision.value & 1u) != 0u) {
        return;
    }
    RbFailureFrame frame = rb_failure.frame;
    RbFailureDrawer drawer = rb_failure.drawer;
    if (rb_failure_revision.value != revision.value) {
        return;
    }
    if (wanted.value != revision.value ||
        frame.address == 0u ||
        frame.visit.value != rb_view_revision.value ||
        frame.address != rb_probe_word(RB_UI_FRAME_CELL)) {
        rb_redraw_consumed = request;
        return;
    }
    if (rb_probe_word(0x009503d0u) != 0u) {
        return;
    }
    RbFailureRedraw code = drawer == RB_FAILURE_DRAW_EDITOR
        ? RB_REDRAW_FAILURE_EDITOR : RB_REDRAW_FAILURE_MENU;
    rb_call_redraw(code);
    if (rb_probe_word(0x009503d0u) == code) {
        rb_redraw_consumed = request;
    }
}
