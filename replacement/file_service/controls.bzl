"""Deliberately wrong owned file-service variants; each must fail a case after reaching the case exit."""

FILE_SERVICE_CONTROLS = [
    "sequence_wrap",
    "skip_allocated",
    "no_free_slot_advance",
    "release_after_failed_wait",
]

def file_service_suffixes():
    return [""] + ["-control-" + control.replace("_", "-") for control in FILE_SERVICE_CONTROLS]
