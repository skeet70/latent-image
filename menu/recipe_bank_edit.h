/* The edit surface's entry contract. menu/recipe_bank_edit.c implements it. */
#ifndef RECIPE_BANK_EDIT_H
#define RECIPE_BANK_EDIT_H

#include <stdint.h>
#include "recipe_bank_ui.h"

/* The EDIT verb's action. */
uint32_t rb_stage_edit_open(uint32_t slot, RbMediaIdentity identity);

/* The edit screen's DISP/BACK: end the session before the stock pop. */
void rb_stage_edit_close(void);

/* The getter dispatch arm for the nine hosting properties. */
uint32_t rb_stage_edit_get(uint32_t host_minus_one);

/* A field list's OK: store the row's code, the col0 word of the pinned stock
 * table row, into the slot; refusals are reported before returning. */
uint32_t rb_stage_edit_row(uint32_t host, uint32_t row);

/* Raise the confirmation dialog for the Reset item. */
uint32_t rb_stage_edit_reset_begin(uint32_t slot, RbMediaIdentity identity);

/* The dialog's OK and CANCEL items and the dialog's own DISP/BACK. */
uint32_t rb_stage_edit_dialog_ok(void);
uint32_t rb_stage_edit_dialog_cancel(void);

#endif
