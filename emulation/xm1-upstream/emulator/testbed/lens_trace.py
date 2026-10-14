"""Passive recorder of lens focus/iris command lifetimes and the committed focus position.

`LensTrace.attach(board)` sets `board.lens_trace`. The lens models report each
command they accept (driver class/body, decoded quantity, direction, timing),
its start and its final physical response (summarised encoder/monitor edges),
and every refusal, in virtual ticks and instructions. The stock committed focus
position (signed 16-bit at 0xccde64) is observed through a chained guest-store
watch, so it records what firmware stores. Nothing here writes guest memory or
changes scheduling. Events are also emitted on `board.testbed_events` when a bus
exists: `lens.command`, `lens.move_start`, `lens.move_end`, `lens.position`
(and `lens.zero`, `lens.iris` with event start/end/cleanup/close, `lens.refusal`).
"""
import json
from pathlib import Path

from . import events

POSITION_ADDRESS = 0xccde64
MAX_RECORDS = 1 << 16


class LensTrace:
    def __init__(self, board):
        self.board = board
        self.records = []
        self.overflow = 0
        self._watch = None

    @classmethod
    def attach(cls, board, *, watch_position=True):
        """Create (or return) the board's trace; `watch_position=False` leaves the board checkpoint-portable."""
        trace = getattr(board, 'lens_trace', None)
        if trace is not None:
            return trace
        trace = board.lens_trace = cls(board)
        if watch_position:
            trace._start_position_watch()
        return trace

    def detach(self):
        if self._watch is not None:
            self._watch.remove()
            self._watch = None
        if getattr(self.board, 'lens_trace', None) is self:
            del self.board.lens_trace

    def _start_position_watch(self):
        from .memory import watch
        position = self.read_position()
        self._append('position', position=position, previous=None, pc=None)
        self._watch = watch(self.board, POSITION_ADDRESS, 2, 'lens.position')
        events.bus_for(self.board).on('mem.write', self._on_write)

    def read_position(self):
        return int.from_bytes(self.board.read_memory(POSITION_ADDRESS, 2), 'little', signed=True)

    def _on_write(self, record):
        if record.get('address') != POSITION_ADDRESS or record.get('label') != 'lens.position':
            return
        old = int.from_bytes(record['old'].to_bytes(2, 'little'), 'little', signed=True)
        new = int.from_bytes(record['new'].to_bytes(2, 'little'), 'little', signed=True)
        self._append('position', position=new, previous=old, pc=record.get('pc'), emit=True)

    def _append(self, kind, emit=False, **data):
        record = dict(kind=kind, ticks=self.board.virtual_ticks, instructions=self.board.instructions, **data)
        if len(self.records) >= MAX_RECORDS:
            del self.records[:MAX_RECORDS // 4]
            self.overflow += MAX_RECORDS // 4
        self.records.append(record)
        if emit:
            fields = {k: v for k, v in data.items()}
            events.emit(self.board, 'lens.' + kind, **fields)
        return record

    def note(self, endpoint, kind, **data):
        """Called by the lens models for accepted commands, motion start/end and refusals."""
        return self._append(kind, emit=True, **data)

    def positions(self):
        """Chronological [(ticks, committed_position)] including the position when attached."""
        return [(r['ticks'], r['position']) for r in self.records if r['kind'] == 'position']

    def of(self, kind):
        return [r for r in self.records if r['kind'] == kind]

    def moves(self):
        """Command lifetimes: each accepted command with its start and end, edges and direction."""
        moves, open_move = [], None
        for r in self.records:
            if r['kind'] == 'command':
                open_move = dict(r, start_tick=None, end_tick=None, edges_delivered=None, monitor_delivered=None)
                moves.append(open_move)
            elif r['kind'] == 'move_start' and open_move is not None:
                open_move['start_tick'] = r['ticks']
            elif r['kind'] == 'move_end' and open_move is not None:
                open_move.update(end_tick=r['ticks'], edges_delivered=r.get('edges'), monitor_delivered=r.get('monitor'))
                open_move = None
        return moves

    def iris_moves(self):
        """Iris lifetimes: [{phase, start_tick, end_tick, quantity, direction, ...}] from lens.iris start/end events."""
        moves, open_move = [], None
        for r in self.records:
            if r['kind'] != 'iris':
                continue
            if r.get('event') == 'start':
                open_move = dict(r, start_tick=r['ticks'], end_tick=None)
                moves.append(open_move)
            elif r.get('event') == 'end' and open_move is not None:
                open_move['end_tick'] = r['ticks']
                open_move = None
        return moves

    def to_json(self):
        return json.dumps(dict(records=self.records, overflow=self.overflow, positions=self.positions()),
                          sort_keys=True, default=str)

    def save(self, path):
        path = Path(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(self.to_json() + '\n')
        return path
