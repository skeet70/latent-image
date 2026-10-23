#include "recipe_bank_diagnostic.h"

#ifdef RB_RAM_DIAGNOSTICS
#ifndef RB_NAME_RECORD
#error "RB_NAME_RECORD must match the emitted recipe package"
#endif

typedef enum {
  RB_DIAG_UNUSED,
  RB_DIAG_ENTERING,
  RB_DIAG_FROZEN,
  RB_DIAG_PUBLISHED
} RbDiagnosticPhase;

typedef enum {
  RB_DIAG_UNOBSERVED,
  RB_DIAG_OBSERVED
} RbDiagnosticObservation;

typedef enum {
  RB_DIAG_NO_FRAME,
  RB_DIAG_REQUESTED_CHILD,
  RB_DIAG_ORIGINAL_PARENT,
  RB_DIAG_OTHER_FRAME
} RbDiagnosticFrameRelation;

typedef struct {
  uint32_t bits;
} RbDiagnosticRowMask;

typedef enum {
  RB_DIAG_CONTEXT_LABEL,
  RB_DIAG_RESULT_LABEL
} RbDiagnosticLabel;

typedef struct {
  uint8_t column;
  uint8_t digits;
} RbDiagnosticHexField;

typedef struct {
  uint32_t address;
} RbDiagnosticAddress;

typedef struct {
  uint32_t status;
} RbDiagnosticIrqMask;

typedef struct {
  RbObservedWord format;
  RbDiagnosticPhase phase;
  RbMenuFrame parent;
  RbMenuFrame child;
  RbObservedWord entry_mode;
  RbObservedWord entry_class;
  RbObservedWord entry_task;
  RbObservedWord entry_epoch;
  RbObservedWord entry_cursor;
  RbObservedWord entry_visible_cursor;
  RbDiagnosticObservation none_seen;
  RbSelectNoneObservation none;
  RbObservedWord none_task;
  RbDiagnosticWalk walk;
  RbDiagnosticRowMask rows_seen;
  enum rb_bank_status verdicts[RB_UI_SELECT_ROWS];
  RbObservedWord transition_result;
  RbMenuFrame returned_frame;
  RbDiagnosticObservation returned_node_seen;
  RbObservedWord returned_id;
  RbObservedWord returned_class;
  RbObservedWord returned_cursor;
  RbObservedWord returned_visible_cursor;
  RbObservedWord returned_depth;
  RbDiagnosticFrameRelation relation;
} RbSelectDiagnostic;

_Static_assert(sizeof(RbSelectDiagnostic) == 148u, "SELECT diagnostic ABI");

volatile RbSelectDiagnostic rb_select_diagnostic;
volatile RbSelectDiscovery rb_select_discovery;
static RbDiagnosticWalk rb_diag_last_walk;

/* The menu hooks share CPU0. */
static inline RbDiagnosticIrqMask rb_diag_lock(void)
{
  RbDiagnosticIrqMask mask;
  __asm__ volatile("mrs %0, cpsr\n\tcpsid i" : "=r"(mask.status) : : "memory");
  return mask;
}

static inline void rb_diag_unlock(RbDiagnosticIrqMask mask)
{
  if ((mask.status & 0x80u) == 0u) {
    __asm__ volatile("cpsie i" : : : "memory");
  }
}

static RbObservedWord rb_diag_read(RbDiagnosticAddress address)
{
  return (RbObservedWord){*(const volatile uint32_t *)(uintptr_t)address.address};
}

static uint32_t rb_discovery_task(void)
{
  uint32_t (*task)(void) = (uint32_t (*)(void))0x00723cb8u;
  return task();
}

static uint32_t rb_discovery_accept(RbDiscoveryRequest request)
{
  return request.walk.sequence != 0u &&
         rb_select_discovery.phase == RB_DISCOVERY_RECORDING &&
         rb_select_discovery.walk.sequence == request.walk.sequence &&
         rb_select_discovery.task.value == request.task.value;
}

static volatile RbDiscoveryCandidate *rb_discovery_candidate(RbDiscoveryRequest request)
{
  if (!rb_discovery_accept(request) || request.file > RB_DISCOVERY_B) {
    return 0;
  }
  return &rb_select_discovery.candidates[request.file];
}

RbDiscoveryRequest rb_discovery_begin(RbDiagnosticWalk walk)
{
  RbDiscoveryRequest request = {{0u}, {0u}, RB_DISCOVERY_NO_FILE};
  if (walk.sequence == 0u) {
    return request;
  }
  RbObservedWord task = {rb_discovery_task()};
  RbDiagnosticIrqMask mask = rb_diag_lock();
  if (rb_select_diagnostic.phase == RB_DIAG_ENTERING &&
      rb_select_discovery.phase == RB_DISCOVERY_UNUSED) {
    rb_select_discovery.format.value = 0x44534331u;
    rb_select_discovery.bytes.value = sizeof(RbSelectDiscovery);
    rb_select_discovery.walk = walk;
    rb_select_discovery.task = task;
    rb_select_discovery.epoch.value = rb_media_epoch();
    rb_select_discovery.winner = RB_DISCOVERY_NO_FILE;
    rb_select_discovery.phase = RB_DISCOVERY_RECORDING;
    request = (RbDiscoveryRequest){walk, task, RB_DISCOVERY_NO_FILE};
  } else if (rb_select_discovery.phase == RB_DISCOVERY_RECORDING) {
    rb_select_discovery.flags.value |= RB_DISCOVERY_OTHER_WALK;
  }
  rb_diag_unlock(mask);
  return request;
}

void rb_discovery_load_begin(RbDiscoveryRequest request, RbMediaIdentity identity)
{
  RbDiagnosticIrqMask mask = rb_diag_lock();
  if (rb_discovery_accept(request)) {
    rb_select_discovery.epoch.value = identity.epoch;
    rb_select_discovery.flags.value |= RB_DISCOVERY_LOAD_STARTED;
  } else if (request.walk.sequence == 0u &&
             rb_select_discovery.phase == RB_DISCOVERY_RECORDING) {
    rb_select_discovery.flags.value |= RB_DISCOVERY_UNTAGGED_LOAD;
  }
  rb_diag_unlock(mask);
}

RbDiscoveryRequest rb_discovery_scan_begin(RbDiscoveryRequest request, RbDiscoveryFile file,
                                           RbMediaIdentity identity, const uint8_t *buffer)
{
  request.file = file;
  RbObservedWord task = {0u};
  if (request.walk.sequence != 0u) {
    task.value = rb_discovery_task();
  }
  RbDiagnosticIrqMask mask = rb_diag_lock();
  volatile RbDiscoveryCandidate *candidate = rb_discovery_candidate(request);
  if (candidate != 0 && candidate->stages.value == 0u) {
    candidate->task = task;
    candidate->epoch.value = identity.epoch;
    candidate->buffer_address.value = (uint32_t)(uintptr_t)buffer;
    candidate->stages.value = RB_DISCOVERY_SCAN;
  } else {
    request.walk.sequence = 0u;
  }
  rb_diag_unlock(mask);
  return request;
}

void rb_discovery_path(RbDiscoveryRequest request, const uint8_t *path)
{
  RbDiagnosticIrqMask mask = rb_diag_lock();
  volatile RbDiscoveryCandidate *candidate = rb_discovery_candidate(request);
  if (candidate != 0) {
    for (uint32_t i = 0u; i < sizeof(candidate->path); i++) {
      candidate->path[i] = path[i];
    }
    candidate->volume.value = path[0];
    candidate->stages.value |= RB_DISCOVERY_PATH;
  }
  rb_diag_unlock(mask);
}

/* Completion fields are shared observations, not callback ownership witnesses. */
void rb_discovery_completion(RbDiscoveryRequest request, RbDiscoveryOperation operation,
                             RbDiscoverySharedCompletion completion)
{
  RbDiagnosticIrqMask mask = rb_diag_lock();
  volatile RbDiscoveryCandidate *candidate = rb_discovery_candidate(request);
  if (candidate != 0) {
    switch (operation) {
    case RB_DISCOVERY_OPEN:
      candidate->open_shared = completion;
      candidate->stages.value |= RB_DISCOVERY_OPEN_RETURN;
      break;
    case RB_DISCOVERY_READ:
      candidate->read_shared = completion;
      candidate->stages.value |= RB_DISCOVERY_READ_RETURN;
      break;
    case RB_DISCOVERY_CLOSE:
      candidate->close_shared = completion;
      candidate->stages.value |= RB_DISCOVERY_CLOSE_RETURN;
      break;
    }
  }
  rb_diag_unlock(mask);
}

void rb_discovery_stage(RbDiscoveryRequest request, RbDiscoveryStage stage,
                        RbObservedWord value, RbObservedWord auxiliary)
{
  RbDiagnosticIrqMask mask = rb_diag_lock();
  volatile RbDiscoveryCandidate *candidate = rb_discovery_candidate(request);
  if (candidate != 0 && (candidate->stages.value & stage) == 0u) {
    switch (stage) {
    case RB_DISCOVERY_OPEN_RESULT:
      candidate->open_result = (enum rb_bank_status)value.value;
      candidate->handle = auxiliary;
      break;
    case RB_DISCOVERY_STAT_RETURN: candidate->stat_return = value; break;
    case RB_DISCOVERY_STAT_SIZE: candidate->stat_size = value; break;
    case RB_DISCOVERY_READ_RESULT:
      candidate->read_result = (enum rb_bank_status)value.value;
      candidate->read_count = auxiliary;
      break;
    case RB_DISCOVERY_BEFORE_CLOSE:
      candidate->before_close_result = (enum rb_bank_status)value.value;
      break;
    case RB_DISCOVERY_CLOSE_RESULT:
      candidate->close_result = (enum rb_bank_status)value.value;
      break;
    case RB_DISCOVERY_CLASS_RESULT:
      candidate->file_class = value;
      candidate->generation = auxiliary;
      break;
    case RB_DISCOVERY_SCAN_RESULT:
      candidate->scan_result = (enum rb_bank_status)value.value;
      break;
    default:
      rb_diag_unlock(mask);
      return;
    }
    candidate->stages.value |= stage;
  }
  rb_diag_unlock(mask);
}

/* Snapshot buffers have one request-local writer.
 * Equal snapshots do not establish immutable classifier input. */
void rb_discovery_snapshot(RbDiscoveryRequest request, const uint8_t *buffer,
                           RbObservedWord bytes, RbDiscoveryStage stage)
{
  if (bytes.value > 1024u ||
      (stage != RB_DISCOVERY_READ_BYTES && stage != RB_DISCOVERY_AFTER_CLASS)) {
    return;
  }
  RbDiagnosticIrqMask mask = rb_diag_lock();
  volatile RbDiscoveryCandidate *candidate = rb_discovery_candidate(request);
  if (candidate == 0 || (candidate->stages.value & stage) != 0u) {
    rb_diag_unlock(mask);
    return;
  }
  rb_diag_unlock(mask);
  volatile uint8_t *out = stage == RB_DISCOVERY_READ_BYTES
                             ? candidate->read_bytes : candidate->after_class;
  for (uint32_t i = 0u; i < bytes.value; i++) {
    out[i] = ((const volatile uint8_t *)buffer)[i];
  }
  mask = rb_diag_lock();
  candidate->snapshot_bytes = bytes;
  if (stage == RB_DISCOVERY_READ_BYTES) {
    candidate->read_epoch.value = rb_media_epoch();
  } else {
    candidate->after_epoch.value = rb_media_epoch();
  }
  candidate->stages.value |= stage;
  rb_diag_unlock(mask);
}

void rb_discovery_winner(RbDiscoveryRequest request, RbDiscoveryFile winner)
{
  RbDiagnosticIrqMask mask = rb_diag_lock();
  if (rb_discovery_accept(request)) {
    rb_select_discovery.winner = winner;
    rb_select_discovery.flags.value |= RB_DISCOVERY_WINNER_REACHED;
  }
  rb_diag_unlock(mask);
}

void rb_discovery_load_end(RbDiscoveryRequest request, enum rb_bank_status status,
                           enum rb_bank_status outcome)
{
  RbDiagnosticIrqMask mask = rb_diag_lock();
  if (rb_discovery_accept(request)) {
    rb_select_discovery.load_return = status;
    rb_select_discovery.load_outcome = outcome;
    rb_select_discovery.flags.value |= RB_DISCOVERY_LOAD_RETURNED;
  }
  rb_diag_unlock(mask);
}

void rb_discovery_end(RbDiscoveryRequest request, enum rb_bank_status status)
{
  RbDiagnosticIrqMask mask = rb_diag_lock();
  if (rb_discovery_accept(request)) {
    rb_select_discovery.none_return = status;
    rb_select_discovery.end_epoch.value = rb_media_epoch();
    rb_select_discovery.phase = RB_DISCOVERY_COMPLETE;
  }
  rb_diag_unlock(mask);
}

void rb_select_diag_arm(RbMenuFrame parent, RbMenuFrame child)
{
  if (rb_select_diagnostic.phase != RB_DIAG_UNUSED ||
      rb_diag_read((RbDiagnosticAddress){child.address + RB_NODE_X}).value != RB_UI_CLASS_SELECT) {
    return;
  }
  uint32_t (*task)(void) = (uint32_t (*)(void))0x00723cb8u;
  rb_select_diagnostic.format.value = 0x53454c31u;
  rb_select_diagnostic.parent = parent;
  rb_select_diagnostic.child = child;
  rb_select_diagnostic.entry_mode = rb_diag_read((RbDiagnosticAddress){0x00956838u});
  rb_select_diagnostic.entry_class = rb_diag_read((RbDiagnosticAddress){0x00951564u});
  rb_select_diagnostic.entry_task.value = task();
  rb_select_diagnostic.entry_epoch.value = rb_media_epoch();
  rb_select_diagnostic.entry_cursor = rb_diag_read((RbDiagnosticAddress){RB_UI_CURSOR_CELL});
  rb_select_diagnostic.entry_visible_cursor = rb_diag_read((RbDiagnosticAddress){RB_UI_VISIBLE_CURSOR_CELL});
  rb_select_diagnostic.phase = RB_DIAG_ENTERING;
}

RbDiagnosticWalk rb_select_diag_walk_begin(void)
{
  RbDiagnosticWalk walk = {0u};
  RbDiagnosticIrqMask mask = rb_diag_lock();
  if (rb_select_diagnostic.phase == RB_DIAG_ENTERING &&
      rb_diag_last_walk.sequence != UINT32_MAX) {
    walk.sequence = ++rb_diag_last_walk.sequence;
  }
  rb_diag_unlock(mask);
  return walk;
}

void rb_select_diag_none(RbDiagnosticWalk walk, RbSelectNoneSource source)
{
  if (walk.sequence == 0u) {
    return;
  }
  uint32_t (*task)(void) = (uint32_t (*)(void))0x00723cb8u;
  RbObservedWord observed_task = {task()};
  RbDiagnosticIrqMask mask = rb_diag_lock();
  if (rb_select_diagnostic.phase == RB_DIAG_ENTERING &&
      rb_select_diagnostic.none_seen == RB_DIAG_UNOBSERVED &&
      (rb_select_discovery.phase == RB_DISCOVERY_UNUSED ||
       rb_select_discovery.walk.sequence == walk.sequence)) {
    rb_select_diagnostic.none = (RbSelectNoneObservation){
        source.status, source.mode, {*source.mode_class},
        {*source.io_status}, {*source.io_value}, {*source.io_done}};
    rb_select_diagnostic.none_task = observed_task;
    rb_select_diagnostic.walk = walk;
    rb_select_diagnostic.verdicts[0] = source.status;
    rb_select_diagnostic.rows_seen.bits = 1u;
    rb_select_diagnostic.none_seen = RB_DIAG_OBSERVED;
  }
  rb_diag_unlock(mask);
}

void rb_select_diag_verdict(RbDiagnosticWalk walk, RbSelection selection,
                            enum rb_bank_status status)
{
  if (walk.sequence == 0u || selection.code >= RB_UI_SELECT_ROWS) {
    return;
  }
  uint32_t bit = 1u << selection.code;
  RbDiagnosticIrqMask mask = rb_diag_lock();
  if (rb_select_diagnostic.phase == RB_DIAG_ENTERING &&
      rb_select_diagnostic.none_seen == RB_DIAG_OBSERVED &&
      rb_select_diagnostic.walk.sequence == walk.sequence &&
      (rb_select_diagnostic.rows_seen.bits & bit) == 0u) {
    rb_select_diagnostic.verdicts[selection.code] = status;
    rb_select_diagnostic.rows_seen.bits |= bit;
  }
  rb_diag_unlock(mask);
}

void rb_select_diag_freeze(RbObservedWord transition_result)
{
  RbDiagnosticIrqMask mask = rb_diag_lock();
  if (rb_select_diagnostic.phase != RB_DIAG_ENTERING) {
    rb_diag_unlock(mask);
    return;
  }
  RbMenuFrame actual = {rb_diag_read((RbDiagnosticAddress){RB_UI_FRAME_CELL}).value};
  RbDiagnosticFrameRelation relation = RB_DIAG_OTHER_FRAME;
  if (actual.address == 0u) {
    relation = RB_DIAG_NO_FRAME;
  } else if (actual.address == rb_select_diagnostic.child.address) {
    relation = RB_DIAG_REQUESTED_CHILD;
  } else if (actual.address == rb_select_diagnostic.parent.address) {
    relation = RB_DIAG_ORIGINAL_PARENT;
  }
  rb_select_diagnostic.transition_result = transition_result;
  rb_select_diagnostic.returned_frame = actual;
  rb_select_diagnostic.relation = relation;
  if (relation == RB_DIAG_REQUESTED_CHILD || relation == RB_DIAG_ORIGINAL_PARENT) {
    rb_select_diagnostic.returned_id = rb_diag_read((RbDiagnosticAddress){actual.address + RB_NODE_ID});
    rb_select_diagnostic.returned_class = rb_diag_read((RbDiagnosticAddress){actual.address + RB_NODE_X});
    rb_select_diagnostic.returned_node_seen = RB_DIAG_OBSERVED;
  }
  rb_select_diagnostic.returned_cursor = rb_diag_read((RbDiagnosticAddress){RB_UI_CURSOR_CELL});
  rb_select_diagnostic.returned_visible_cursor = rb_diag_read((RbDiagnosticAddress){RB_UI_VISIBLE_CURSOR_CELL});
  rb_select_diagnostic.returned_depth = rb_diag_read((RbDiagnosticAddress){0x009575f4u});
  rb_select_diagnostic.phase = RB_DIAG_FROZEN;
  rb_diag_unlock(mask);
}

/* Diagnostic RAM runs reserve CUSTOM 1..2's inline labels until power-off. */
static uint16_t *rb_diag_label(RbDiagnosticLabel label)
{
  return (uint16_t *)(uintptr_t)(RB_NAME_RECORD + (3u + (uint32_t)label) * 0x42u);
}

static void rb_diag_template(RbDiagnosticLabel label, const char *text)
{
  uint16_t *buffer = rb_diag_label(label);
  uint32_t i = 0u;
  while (i < 14u && text[i] != '\0') {
    buffer[i] = (uint8_t)text[i];
    i++;
  }
  while (i < 15u) {
    buffer[i++] = 0u;
  }
}

static void rb_diag_hex(RbDiagnosticLabel label, RbDiagnosticHexField field,
                        RbObservedWord word)
{
  uint16_t *buffer = rb_diag_label(label);
  if (field.digits < 8u && (word.value >> (4u * field.digits)) != 0u) {
    return;
  }
  for (uint32_t i = field.digits; i != 0u; i--) {
    uint32_t digit = word.value & 15u;
    buffer[field.column + i - 1u] = (uint16_t)(digit < 10u ? '0' + digit : 'A' + digit - 10u);
    word.value >>= 4u;
  }
}

void rb_select_diag_publish(void)
{
  if (rb_select_diagnostic.phase != RB_DIAG_FROZEN) {
    return;
  }
  rb_diag_template(RB_DIAG_CONTEXT_LABEL, "??????????????");
  rb_diag_template(RB_DIAG_RESULT_LABEL, "?????????????");
  if (rb_select_diagnostic.none_seen == RB_DIAG_OBSERVED) {
    rb_diag_hex(RB_DIAG_CONTEXT_LABEL, (RbDiagnosticHexField){0u, 2u},
                (RbObservedWord){rb_select_diagnostic.none.status});
    rb_diag_hex(RB_DIAG_CONTEXT_LABEL, (RbDiagnosticHexField){2u, 2u}, rb_select_diagnostic.none.mode);
    rb_diag_hex(RB_DIAG_CONTEXT_LABEL, (RbDiagnosticHexField){4u, 1u}, rb_select_diagnostic.none.mode_class);
    rb_diag_hex(RB_DIAG_CONTEXT_LABEL, (RbDiagnosticHexField){5u, 1u}, rb_select_diagnostic.none.io_done);
    rb_diag_hex(RB_DIAG_CONTEXT_LABEL, (RbDiagnosticHexField){6u, 8u}, rb_select_diagnostic.none.io_status);
    rb_diag_hex(RB_DIAG_RESULT_LABEL, (RbDiagnosticHexField){0u, 2u}, rb_select_diagnostic.none_task);
    rb_diag_hex(RB_DIAG_RESULT_LABEL, (RbDiagnosticHexField){2u, 8u}, rb_select_diagnostic.none.io_value);
  }
  if (rb_select_diagnostic.rows_seen.bits == 0xffu) {
    RbDiagnosticRowMask disabled = {0u};
    for (uint32_t row = 0u; row < RB_UI_SELECT_ROWS; row++) {
      if (rb_select_diagnostic.verdicts[row] != RB_OK) {
        disabled.bits |= 1u << row;
      }
    }
    rb_diag_hex(RB_DIAG_RESULT_LABEL, (RbDiagnosticHexField){10u, 2u},
                (RbObservedWord){disabled.bits});
  }
  static const char relation[] = "NCPO";
  rb_diag_label(RB_DIAG_RESULT_LABEL)[12] = (uint16_t)relation[rb_select_diagnostic.relation];
  rb_select_diagnostic.phase = RB_DIAG_PUBLISHED;
  rb_status_present((RbMessageLabels){{0x840u}, {0x83fu}, {0u}});
}
#endif
