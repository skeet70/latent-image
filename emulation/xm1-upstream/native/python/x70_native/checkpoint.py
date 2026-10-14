"""Existing strict whole-board rollback, with native ownership identities."""
from emulator import checkpoint as reference
from .board import RustBoard


class NativeCheckpoint(reference.BoardCheckpoint):
    def __init__(self, board):
        _require_native_modes_disabled(board)
        super().__init__(board)
        self.metadata["limits"].append(
            "Native routine auditing and fetch guards must remain disabled; their state and hooks are not checkpointed")

    def restore(self, board=None):
        board = self._board if board is None else board
        _require_native_modes_disabled(board)
        return super().restore(board)


def _require_native_modes_disabled(board):
    """Refuse native state omitted from rollback, before any memory mutation.

    Older extensions can omit either optional API. Once enabled, these native
    hooks/state persist outside Unicorn contexts and are not rollback state.
    """
    machine = getattr(board, "_native", None)
    observer = getattr(machine, "access_probe_enabled", None)
    if observer is not None and observer():
        raise reference.CheckpointError(
            "Native access probe is active; checkpoints omit its records/hooks")
    for api, label in (("audit_state", "audit"), ("fetch_guard_state", "fetch guard")):
        state = getattr(machine, api, None)
        if state is None:
            continue
        for core in range(len(board.cpus)):
            if state(core)["enabled"]:
                raise reference.CheckpointError(
                    f"Native {label} mode is active on CPU{core}; checkpoints omit its state and hooks")


def capture(board):
    if not isinstance(board, RustBoard):
        raise TypeError("Native checkpoint requires a RustBoard")
    return NativeCheckpoint(board)


def semantic_fingerprint(board):
    """Fingerprint the Rust machine and its stored platform/device state."""
    return reference.fingerprint(board)
