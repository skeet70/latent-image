"""Testbed: run control, waits, memory watches and guest input over a loaded board."""
from dataclasses import dataclass

from . import events, memory, patch, predicates
from .display import DisplayRecorder
from .inputs import KEY_EVENTS, EMIT_KEY, ScannerStub

SNAPSHOT_MODELS = ('emulator.afs_measurement_data',)


@dataclass
class RunResult:
    matched: bool | None    # None for a plain run
    instructions: int       # elapsed during this call
    ticks: int
    reason: str             # done | matched | timeout | failure


class Testbed:
    def __init__(self, board):
        self.board = board
        self.bus = events.bus_for(board)
        self.stub = ScannerStub.for_board(board)
        self.display = DisplayRecorder(self)

    @classmethod
    def load(cls, snapshot, allowed_revisions=SNAPSHOT_MODELS):
        """Load a portable snapshot, tolerating reviewed revisions of the named model modules."""
        from tools.afs_load import load_dev
        return cls(load_dev(snapshot, allowed=tuple(allowed_revisions)))

    # Execution
    def _advance(self, count):
        board = self.board
        if board.failure is not None:
            return
        board.run(count + 2 * board.quantum, pause_after=board.instructions + count)

    def _result(self, start, matched, reason):
        board = self.board
        if board.failure is not None:
            reason = 'failure'
        return RunResult(matched, board.instructions - start[0], board.virtual_ticks - start[1], reason)

    def run(self, instructions):
        """Run about `instructions` (a quantum may overshoot); stops on board.failure."""
        start = (self.board.instructions, self.board.virtual_ticks)
        self._advance(instructions)
        return self._result(start, None, 'done')

    def run_until(self, predicate, timeout_instructions, step=100_000):
        """Run in `step` chunks until predicate(testbed) is true, timeout or failure."""
        predicate = predicates.wrap(predicate)
        board = self.board
        start = (board.instructions, board.virtual_ticks)
        predicate.begin(self)
        try:
            while True:
                if predicate(self):
                    return self._result(start, True, 'matched')
                spent = board.instructions - start[0]
                if board.failure is not None or spent >= timeout_instructions:
                    return self._result(start, False, 'timeout')
                self._advance(min(step, timeout_instructions - spent))
        finally:
            predicate.end(self)

    def wait_event(self, name, timeout_instructions, where=None, step=100_000):
        return self.run_until(predicates.event(name, where), timeout_instructions, step)

    # Observation
    def read(self, address, size=4):
        return int.from_bytes(bytes(self.board.read_memory(address, size, cpu_id=0)), 'little')

    def watch(self, address, size, name=None):
        return memory.watch(self.board, address, size, name)

    # Guest-context input
    def debug_key(self, name, state, *, allow_side_effects=False):
        """Queue an event; use Buttons/EmulatorKeys for guarded modes and CMD.

        The returned ticket means emission, not that the UI processed the key.
        """
        from tools.x70_buttons import validate_emit
        entry, state = validate_emit(name, state, allow_side_effects)
        if entry.service_guard or entry.kind == 'CMD':
            raise PermissionError('Use the shared Buttons API with a service guard')
        key = entry.name
        code = KEY_EVENTS[key, state]
        ticket = self.stub.submit(EMIT_KEY, (code, 0), auto_free=True)
        self.bus.emit('input.debug_key', key=key, state=state, event=code, slot=ticket.slot)
        return ticket

    def host_call(self, function, *args, timeout_instructions=2_000_000, step=20_000):
        """Run a guest function from the scanner context and return r0 (waits by running)."""
        ticket = self.stub.submit(function, args)
        self.bus.emit('input.host_call', function=function, args=args, slot=ticket.slot)
        result = self.run_until(lambda tb: ticket.done, timeout_instructions, step)
        if not result.matched:
            raise TimeoutError(f'host_call {function:#x} did not complete ({result.reason})')
        return ticket.result()

    def install_ram_patch(self, code, base, word_swaps=(), reserve=None):
        return patch.install_ram_patch(self.board, code, base, word_swaps, reserve)

    def screenshot(self, output='lcd'):
        """Screenshot of 'lcd', 'hdmi' or an image instance (see display.py); `.png` holds the PNG bytes."""
        return self.display.screenshot(output)

