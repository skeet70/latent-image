#ifndef RECIPE_BANK_DIAGNOSTIC_H
#define RECIPE_BANK_DIAGNOSTIC_H

#ifdef RB_RAM_DIAGNOSTICS
#include "recipe_bank_ui.h"

typedef struct {
  uint32_t value;
} RbObservedWord;

typedef struct {
  enum rb_bank_status status;
  RbObservedWord mode;
  RbObservedWord mode_class;
  RbObservedWord io_status;
  RbObservedWord io_value;
  RbObservedWord io_done;
} RbSelectNoneObservation;

typedef struct {
  enum rb_bank_status status;
  RbObservedWord mode;
  const volatile uint32_t *mode_class;
  const volatile uint32_t *io_status;
  const volatile uint32_t *io_value;
  const volatile uint32_t *io_done;
} RbSelectNoneSource;

typedef enum {
  RB_DISCOVERY_UNUSED,
  RB_DISCOVERY_RECORDING,
  RB_DISCOVERY_COMPLETE
} RbDiscoveryPhase;

typedef enum {
  RB_DISCOVERY_A,
  RB_DISCOVERY_B,
  RB_DISCOVERY_NO_FILE = 255
} RbDiscoveryFile;

typedef enum {
  RB_DISCOVERY_SCAN = 1u << 0,
  RB_DISCOVERY_PATH = 1u << 1,
  RB_DISCOVERY_OPEN_RETURN = 1u << 2,
  RB_DISCOVERY_OPEN_RESULT = 1u << 3,
  RB_DISCOVERY_STAT_RETURN = 1u << 4,
  RB_DISCOVERY_STAT_SIZE = 1u << 5,
  RB_DISCOVERY_READ_RETURN = 1u << 6,
  RB_DISCOVERY_READ_RESULT = 1u << 7,
  RB_DISCOVERY_BEFORE_CLOSE = 1u << 8,
  RB_DISCOVERY_CLOSE_RETURN = 1u << 9,
  RB_DISCOVERY_CLOSE_RESULT = 1u << 10,
  RB_DISCOVERY_READ_BYTES = 1u << 11,
  RB_DISCOVERY_AFTER_CLASS = 1u << 12,
  RB_DISCOVERY_CLASS_RESULT = 1u << 13,
  RB_DISCOVERY_SCAN_RESULT = 1u << 14
} RbDiscoveryStage;

typedef enum {
  RB_DISCOVERY_OTHER_WALK = 1u << 0,
  RB_DISCOVERY_UNTAGGED_LOAD = 1u << 1,
  RB_DISCOVERY_LOAD_STARTED = 1u << 2,
  RB_DISCOVERY_LOAD_RETURNED = 1u << 3,
  RB_DISCOVERY_WINNER_REACHED = 1u << 4
} RbDiscoveryFlag;

typedef enum {
  RB_DISCOVERY_OPEN,
  RB_DISCOVERY_READ,
  RB_DISCOVERY_CLOSE
} RbDiscoveryOperation;

typedef struct {
  RbDiagnosticWalk walk;
  RbObservedWord task;
  RbDiscoveryFile file;
} RbDiscoveryRequest;

typedef struct {
  RbObservedWord producer_return;
  RbObservedWord status;
  RbObservedWord value;
  RbObservedWord done;
} RbDiscoverySharedCompletion;

typedef struct {
  RbObservedWord stages;
  RbObservedWord task;
  RbObservedWord epoch;
  RbObservedWord buffer_address;
  uint8_t path[16];
  RbObservedWord volume;
  RbDiscoverySharedCompletion open_shared;
  enum rb_bank_status open_result;
  RbObservedWord handle;
  RbObservedWord stat_return;
  RbObservedWord stat_size;
  RbDiscoverySharedCompletion read_shared;
  enum rb_bank_status read_result;
  RbObservedWord read_count;
  enum rb_bank_status before_close_result;
  RbDiscoverySharedCompletion close_shared;
  enum rb_bank_status close_result;
  RbObservedWord file_class;
  RbObservedWord generation;
  enum rb_bank_status scan_result;
  RbObservedWord snapshot_bytes;
  RbObservedWord read_epoch;
  RbObservedWord after_epoch;
  uint8_t read_bytes[1024];
  uint8_t after_class[1024];
} RbDiscoveryCandidate;

typedef struct {
  RbObservedWord format;
  RbObservedWord bytes;
  RbDiscoveryPhase phase;
  RbDiagnosticWalk walk;
  RbObservedWord task;
  RbObservedWord epoch;
  RbObservedWord flags;
  enum rb_bank_status load_return;
  enum rb_bank_status load_outcome;
  RbDiscoveryFile winner;
  enum rb_bank_status none_return;
  RbObservedWord end_epoch;
  RbDiscoveryCandidate candidates[2];
} RbSelectDiscovery;

_Static_assert(sizeof(RbDiscoveryCandidate) == 2188u, "discovery candidate ABI");
_Static_assert(sizeof(RbSelectDiscovery) == 4424u, "SELECT discovery ABI");

extern volatile RbSelectDiscovery rb_select_discovery;

RbDiscoveryRequest rb_discovery_begin(RbDiagnosticWalk walk);
void rb_discovery_load_begin(RbDiscoveryRequest request, RbMediaIdentity identity);
RbDiscoveryRequest rb_discovery_scan_begin(RbDiscoveryRequest request, RbDiscoveryFile file,
                                           RbMediaIdentity identity, const uint8_t *buffer);
void rb_discovery_path(RbDiscoveryRequest request, const uint8_t *path);
void rb_discovery_completion(RbDiscoveryRequest request, RbDiscoveryOperation operation,
                             RbDiscoverySharedCompletion completion);
void rb_discovery_stage(RbDiscoveryRequest request, RbDiscoveryStage stage,
                        RbObservedWord value, RbObservedWord auxiliary);
void rb_discovery_snapshot(RbDiscoveryRequest request, const uint8_t *buffer,
                           RbObservedWord bytes, RbDiscoveryStage stage);
void rb_discovery_winner(RbDiscoveryRequest request, RbDiscoveryFile winner);
void rb_discovery_load_end(RbDiscoveryRequest request, enum rb_bank_status status,
                           enum rb_bank_status outcome);
void rb_discovery_end(RbDiscoveryRequest request, enum rb_bank_status status);

void rb_select_diag_arm(RbMenuFrame parent, RbMenuFrame child);
RbDiagnosticWalk rb_select_diag_walk_begin(void);
void rb_select_diag_none(RbDiagnosticWalk walk, RbSelectNoneSource source);
void rb_select_diag_verdict(RbDiagnosticWalk walk, RbSelection selection, enum rb_bank_status status);
void rb_select_diag_freeze(RbObservedWord transition_result);
void rb_select_diag_publish(void);
#endif

#endif
