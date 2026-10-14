"""Synchronous event bus attached to a board as `board.testbed_events`.

Observation only: emitting never touches guest memory or scheduling. Device
models call `emit(board, name, **data)`, which is a no-op without a bus.
"""
from collections import deque

HISTORY = 4096


class EventBus:
    def __init__(self, board=None, history=HISTORY):
        self.board = board
        self.history = deque(maxlen=history)
        self.seq = 0
        self._subscribers = {}

    def emit(self, name, **data):
        board = self.board
        self.seq += 1
        record = {'name': name, 'seq': self.seq,
                  'instructions': getattr(board, 'instructions', 0),
                  'ticks': getattr(board, 'virtual_ticks', 0), **data}
        self.history.append(record)
        for key in (name, '*'):
            for callback in tuple(self._subscribers.get(key, ())):
                callback(record)
        return record

    def on(self, name, callback):
        """Subscribe to one event name or '*'; returns the callback."""
        self._subscribers.setdefault(name, []).append(callback)
        return callback

    def off(self, name, callback):
        callbacks = self._subscribers.get(name, [])
        if callback in callbacks:
            callbacks.remove(callback)

    def events(self, name=None, since=0, where=None):
        """Retained records with seq > since, optionally by name and predicate."""
        return [r for r in self.history if r['seq'] > since and (name is None or r['name'] == name)
                and (where is None or where(r))]


def bus_for(board):
    """The board's bus, created on first use."""
    bus = getattr(board, 'testbed_events', None)
    if bus is None:
        bus = board.testbed_events = EventBus(board)
    return bus


def emit(board, name, **data):
    """Cheap hook for device models: no-op unless a bus exists."""
    bus = getattr(board, 'testbed_events', None)
    return bus.emit(name, **data) if bus is not None else None
