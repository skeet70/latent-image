"""Composable wait predicates for `Testbed.run_until`.

A predicate is called with the Testbed as `pred(tb)` between run steps. State
predicates are sampled, so a transient shorter than the step can be missed
(use a smaller `step`, `event` or `mem_changed`). `event` and `mem_changed`
are sticky: true once the change was seen after the wait began. Combine with
`&`, `|` and `~`; plain callables `fn(tb)` are accepted by run_until.
"""

REC_STATE_BASE = 0x1624240      # REC FSM state halfwords, 2*machine
LOWER_STATE_BASE = 0x15f87f0    # SACLA lower machine halfwords, 2*machine
UI_MODE = 0xcbd3e0


class Pred:
    def begin(self, tb):
        pass

    def end(self, tb):
        pass

    def __call__(self, tb):
        raise NotImplementedError

    def __and__(self, other):
        return All(self, other)

    def __or__(self, other):
        return Any(self, other)

    def __invert__(self):
        return Not(self)


class Fn(Pred):
    def __init__(self, fn):
        self.fn = fn

    def __call__(self, tb):
        return bool(self.fn(tb))


def wrap(value):
    return value if isinstance(value, Pred) else Fn(value)


class _Group(Pred):
    def __init__(self, *parts):
        self.parts = [p for part in parts for p in (part.parts if type(part) is type(self) else [wrap(part)])]

    def begin(self, tb):
        for part in self.parts:
            part.begin(tb)

    def end(self, tb):
        for part in self.parts:
            part.end(tb)


class All(_Group):
    def __call__(self, tb):
        return all([part(tb) for part in self.parts])      # evaluate all: sticky parts must observe


class Any(_Group):
    def __call__(self, tb):
        return any([part(tb) for part in self.parts])


class Not(Pred):
    def __init__(self, part):
        self.part = wrap(part)

    def begin(self, tb):
        self.part.begin(tb)

    def end(self, tb):
        self.part.end(tb)

    def __call__(self, tb):
        return not self.part(tb)


def mem_pred(address, size, fn):
    """True when fn(unsigned little-endian value at address) is truthy."""
    return Fn(lambda tb: fn(tb.read(address, size)))


def mem_equals(address, value, size=4):
    return mem_pred(address, size, lambda v: v == value)


def rec_state(machine, state):
    return mem_equals(REC_STATE_BASE + 2 * machine, state, 2)


def lower_state(machine, state):
    return mem_equals(LOWER_STATE_BASE + 2 * machine, state, 2)


def ui_mode(n):
    return mem_equals(UI_MODE, n, 4)


def device_attr(type_name, attr, value_or_fn):
    """Device selected by class name (unique); attr may be dotted. Compare by == or predicate."""
    def check(tb):
        matches = [d for d in tb.board.devices if type(d).__name__ == type_name]
        if len(matches) != 1:
            raise ValueError(f'{type_name}: expected one device, found {len(matches)}')
        value = matches[0]
        for part in attr.split('.'):
            value = getattr(value, part)
        return value_or_fn(value) if callable(value_or_fn) else value == value_or_fn
    return Fn(check)


class event(Pred):
    """A bus event (optionally `where(record)`) emitted after the wait began."""
    def __init__(self, name, where=None):
        self.name, self.where, self.seen, self.tb = name, where, False, None

    def begin(self, tb):
        self.seen, self.tb = False, tb
        tb.bus.on(self.name, self._on)

    def end(self, tb):
        tb.bus.off(self.name, self._on)

    def _on(self, record):
        if self.where is None or self.where(record):
            self.seen = True

    def __call__(self, tb):
        return self.seen


class mem_changed(Pred):
    """Any guest store changing the bytes after the wait began (uses a memory watch)."""
    def __init__(self, address, size):
        self.address, self.size, self.seen, self.watch = address, size, False, None

    def begin(self, tb):
        self.seen = False
        self.watch = tb.watch(self.address, self.size)
        tb.bus.on('mem.write', self._on)

    def end(self, tb):
        tb.bus.off('mem.write', self._on)
        if self.watch is not None:
            self.watch.remove()
            self.watch = None

    def _on(self, record):
        if record['address'] == self.address and record['size'] == self.size:
            self.seen = True

    def __call__(self, tb):
        return self.seen
