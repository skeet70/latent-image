/* The bank's arm bodies. menu/recipe_bank_arm.S is what the image-5 commit
 * dispatcher jumps to for the bank's properties; these functions are its
 * callees. RB_* addresses are placement values the emitted plan fixes and the
 * build passes as -D macros.
 *
 * The storage lane's kernel does the record work: rb_recall, rb_save_current,
 * rb_reset, rb_readback and rb_name_commit. This file holds the arm entry
 * semantics, the label-record refresh and the dispatcher's notification.
 */
#include <stdint.h>

#include "recipe_bank_ui.h"

#ifndef RB_NAME_RECORD
#define RB_NAME_RECORD 0x011dcca0u
#endif

#define RB_PAYLOAD 0x28u
#define RB_NAME 26u
#define RB_RECORD 0x42u

/* The accessor indexes a record as base + (id - 0x83c) * 0x42
   (reports/special-label-resolution.md), so RB_NAME_RECORD is record 0, id
   0x83c, and the bank's seven records start after the three stock ones. */
#define RB_STOCK_RECORDS 3u

/* Storage lane's kernel (candidate, non-installable). */
extern uint32_t rb_recall(uint32_t slot);
extern uint32_t rb_save_current(uint32_t slot, const uint8_t *name26);
extern uint32_t rb_reset(uint32_t slot);
extern uint32_t rb_readback(uint32_t slot, uint8_t *name_out, uint8_t *payload_out);
extern uint32_t rb_load(void);
extern uint32_t rb_name_commit(uint32_t slot, const uint8_t *name26);
extern uint32_t rb_name_resident(uint32_t slot, uint8_t *name26);

/* The label record's inline buffer only; its layout halfwords stay. */
static void rb_name_show(uint32_t slot, const uint8_t *name)
{
    uint8_t *record = (uint8_t *)(RB_NAME_RECORD + (RB_STOCK_RECORDS + slot) * RB_RECORD);
    for (uint32_t i = 0u; i < RB_NAME; i++) {
        record[i] = name[i];
    }
}

/* Called by rb_name_edit_commit when the stock editor confirms. The storage op
 * is name-only: it preserves the slot's committed payload, and the staged-edit
 * buffers are untouched apart from refreshing a same-slot staged name. */
uint32_t rb_name_write(uint32_t slot, const uint8_t *units)
{
    if (slot >= RB_BANK_SLOTS) {
        return 1u;
    }
    uint8_t name[RB_NAME];
    for (uint32_t i = 0u; i < 2u * 12u; i++) {
        name[i] = units[i];
    }
    name[2u * 12u] = 0u;
    name[2u * 12u + 1u] = 0u;
    uint32_t status = rb_name_commit(slot, name);
    if (status != 0u) {
        return status;
    }
    rb_name_show(slot, name);
    return 0u;
}

/* The page discovery's seven-name refresh: slot 0 opens a visit and owns the
 * one rb_load a visit may make, every row then reads the resident image, and a
 * failed load latches for the rest of the visit. The names come from one bank
 * image, and the RAM re-read picks up a rename or a reset without treating
 * either as a media identity change. */
static uint8_t rb_slot_names[RB_BANK_SLOTS][RB_NAME];
static uint32_t rb_slot_names_epoch = 0xffffffffu;
static uint32_t rb_slot_names_ready = 0u;
static uint32_t rb_discovery_status = RB_OK;

/* The resident image's names, RAM only; RB_UNSET leaves an entry zero-filled. */
static void rb_slot_names_read(void)
{
    uint32_t s;
    for (s = 0u; s < RB_BANK_SLOTS; s++) {
        (void)rb_name_resident(s, rb_slot_names[s]);
    }
}

uint32_t rb_slot_load(uint32_t slot)
{
    if (slot >= RB_BANK_SLOTS) {
        return RB_RANGE;
    }
    if (slot == 0u) {
        if (rb_slot_names_ready == 0u || rb_slot_names_epoch != rb_media_epoch()) {
            uint32_t status = rb_load();
            if (status != RB_OK && status != RB_NO_BANK && status != RB_INVALID) {
                rb_discovery_status = status;
                return status;
            }
            rb_slot_names_epoch = rb_media_epoch();
            rb_slot_names_ready = 1u;
        }
        rb_discovery_status = RB_OK;
    } else if (rb_discovery_status != RB_OK) {
        return rb_discovery_status;
    } else if (rb_slot_names_ready == 0u) {
        return RB_UNSET;
    }
    rb_slot_names_read();
    rb_name_show(slot, rb_slot_names[slot]);
    return RB_OK;
}

/*
 * On-camera rename binding for the stock text-entry widget (X-M1 1.50).
 *
 * Editor rows commit through image-5 FUN_00d4ad38 (0x00d4ad38): row 0x27
 * cursor left, 0x28 cursor right, 0x29 delete, 0x2a confirm, and every
 * other row inserts its conversion col0 through FUN_00d4a2a0. Node ids
 * 0x4d/0x56 stage into the 24-byte buffer at 0x954394 with cursor floor
 * 9, and their confirm writes wireless properties 0xa8/0xa9; any other
 * node id stages into the 14-byte buffer below with cursor floor 0, and
 * its stock confirm writes nothing. The bank's editor node carries a bank
 * node id, so the wireless name is unreachable from this surface.
 *
 * The stock opens of this flavor are FUN_00c72844 and FUN_00c75cdc
 * (class 0x130, row 0): modal-75 state word, cursor, buffer load, then
 * the repaint request and its flag. The buffer-B opens FUN_00c7ddac and
 * FUN_00c80940 write the modal-76 state word 0x01467620 = 0x24 instead;
 * that is the other flavor and not this one's mode.
 */
#define RB_EDIT_BUF 0x00954385u
#define RB_EDIT_CUR 0x00954384u
#define RB_EDIT_CAP 14u
#define RB_NAME_UNITS 12u
#define RB_EDIT_MODE 0x0146761eu
#define RB_EDIT_MODE_OPEN 0x1eu
#define RB_EDIT_DIRTY 0x009504a8u
#define RB_EDIT_REDRAW 0x92au

static uint32_t rb_edit_slot = 0xffffffffu;

extern void rb_call_redraw(uint32_t region);

/* The buffer is stock RAM and lives exactly as long as the editor screen;
 * nothing is staged in the bank until confirm. */
uint32_t rb_name_edit_open(uint32_t slot)
{
    if (slot >= RB_BANK_SLOTS) {
        return 1u;
    }
    uint8_t name[RB_NAME];
    uint8_t payload[RB_PAYLOAD];
    uint32_t status = rb_readback(slot, name, payload);
    if (status != 0u) {
        return status;
    }
    uint8_t *buf = (uint8_t *)RB_EDIT_BUF;
    for (uint32_t i = 0u; i < RB_EDIT_CAP; i++) {
        buf[i] = 0u;
    }
    uint32_t n = 0u;
    while (n < RB_NAME_UNITS) {
        uint8_t lo = name[2u * n];
        uint8_t hi = name[2u * n + 1u];
        if ((lo == 0u && hi == 0u) || hi != 0u || lo < 0x20u || 0x7eu < lo) {
            break;
        }
        buf[n] = lo;
        n++;
    }
    *(volatile uint16_t *)RB_EDIT_MODE = (uint16_t)RB_EDIT_MODE_OPEN;
    *(volatile uint8_t *)RB_EDIT_CUR = 0u;
    *(volatile uint32_t *)RB_EDIT_DIRTY = 1u;
    rb_call_redraw(RB_EDIT_REDRAW);
    rb_edit_slot = slot;
    return 0u;
}

/* Editor confirm, the stock row-0x2a action for a bank node id. The
 * widget's right-move pads with spaces and its stock normalizer drops
 * leading and trailing ones, so both ends are trimmed here before the
 * ASCII bytes become the name's twelve UTF-16LE units. More than twelve
 * staged characters is refused whole rather than truncated. */
uint32_t rb_name_edit_commit(void)
{
    uint32_t slot = rb_edit_slot;
    if (slot >= RB_BANK_SLOTS) {
        return 1u;
    }
    const uint8_t *buf = (const uint8_t *)RB_EDIT_BUF;
    uint32_t n = 0u;
    while (n < RB_EDIT_CAP && buf[n] != 0u) {
        n++;
    }
    uint32_t first = 0u;
    while (first < n && buf[first] == ' ') {
        first++;
    }
    while (n > first && buf[n - 1u] == ' ') {
        n--;
    }
    if (n - first > RB_NAME_UNITS) {
        /* RB_RANGE maps to the cannot-execute message; RB_NOT_READY (1) maps to
         * the no-card text, which is wrong for an over-length name. */
        return RB_RANGE;
    }
    uint8_t units[2u * RB_NAME_UNITS];
    for (uint32_t i = 0u; i < 2u * RB_NAME_UNITS; i++) {
        units[i] = 0u;
    }
    for (uint32_t i = first; i < n; i++) {
        units[2u * (i - first)] = buf[i];
    }
    return rb_name_write(slot, units);
}

/* Editor cancel: drop the staged slot without touching the bank. The
 * staged bytes are stock RAM and are reloaded on the next open. */
uint32_t rb_name_edit_cancel(void)
{
    rb_edit_slot = 0xffffffffu;
    return 0u;
}


