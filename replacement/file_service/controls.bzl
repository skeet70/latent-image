"""Deliberately wrong owned file-service variants; each must fail a case after reaching the case exit."""

FILE_SERVICE_CONTROLS = [
    "sequence_wrap",
    "skip_allocated",
    "no_free_slot_advance",
    "release_after_failed_wait",
    "translate_unknown_ok",
    "readiness_absent_ok",
    "result_release_keeps_owner",
    "open_failure_keeps_tag",
    "defer_no_count",
    "intercept_keeps_callback",
    "media_task_no_busy",
    "media_task_worker_row",
    "close_keeps_tag",
    "open_failure_keeps_pair",
    "write_count_unstored",
    "flagged_close_unmarked",
]

def file_service_suffixes():
    return [""] + ["-control-" + control.replace("_", "-") for control in FILE_SERVICE_CONTROLS]
