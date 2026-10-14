/* The staged-edit surface's entry contract. menu/recipe_bank_edit.c implements
 * it. */
#ifndef RECIPE_BANK_EDIT_H
#define RECIPE_BANK_EDIT_H

#include <stdint.h>
#include "recipe_bank_ui.h"

/* The EDIT verb's action. */
uint32_t rb_stage_edit_open(uint32_t slot);

/* The setter and getter dispatch arms for the nine hosting properties; the
 * getter arm receives property - 1. */
uint32_t rb_stage_edit_set(uint32_t host, uint32_t code);
uint32_t rb_stage_edit_get(uint32_t host_minus_one);

/* The row-token staging entry for an interposer-routed row select: the row's
 * stored code is the col0 word of the pinned stock table row. */
uint32_t rb_stage_edit_row(uint32_t host, uint32_t row);

/* One parameter item's DISP/BACK: an edited item raises its own OK/CANCEL
 * dialog; an untouched item returns RB_NOT_READY and the caller runs the stock
 * pop. */
uint32_t rb_stage_edit_item_leave(void);

/* The edit screen's DISP/BACK leaving edge: raise the confirmation dialog. */
uint32_t rb_stage_edit_leave(void);

/* Raise the confirmation dialog for one pending action; the Save Current and
 * Reset items use this before their bank action runs. */
uint32_t rb_stage_edit_confirm_begin(enum rb_edit_confirm what, uint32_t slot);

/* The dialog's OK and CANCEL items and the dialog's own DISP/BACK. */
uint32_t rb_stage_edit_dialog_ok(void);
uint32_t rb_stage_edit_dialog_cancel(void);

#endif
