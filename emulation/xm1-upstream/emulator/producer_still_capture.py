"""Explicit still numeric owner for stock 1310E98 initialization.

The initializer shares the audited reset/idle/desired/22-access hardware
transaction. Admission binds the observed generation9 Source1 clock and
S2 descriptor. Sparse native tests declare their prior caller separately;
this owner supplies no pixels, IRQ or capture result.
"""
from copy import copy, deepcopy
import struct

from .hardware import UnsupportedDevice
from . import histogram_protocol as hp
from . import producer_afs_focus as af
from . import producer_capture_metstats as capture


PROFILE = 'declared-afs-still-numeric-initialization-unmeasured'
KEY = 'still_capture'
CLOCK_PROFILE = 'declared-afs-generation9-still-initialization-source1-clock-unmeasured'
CLOCK_KEY = 'still_initialization'


def _owner(device):
    return device.rearm.get(KEY) if type(device.rearm) is dict else None


def _record(device, terminal, clock=None, caller=None):
    return dict(profile=PROFILE, cpu=0, armed_tick=device.board.virtual_ticks,
                terminal=deepcopy(terminal), terminal_sha256=hp.digest(terminal),
                clock=deepcopy(clock), clock_sha256=hp.digest(clock),
                caller=deepcopy(caller), caller_sha256=hp.digest(caller),
                state=af._state(terminal['registers']), accesses=[])


def arm(device, profile=PROFILE):
    """Bind a future real initialization clock; produce no guest effects."""
    if profile != PROFILE or type(profile) is not str:
        raise UnsupportedDevice('Unknown still numeric producer profile')
    if type(device.rearm) is not dict or KEY in device.rearm or capture.KEY not in device.rearm:
        raise UnsupportedDevice('Still numeric admission awaits the actual S2 source and descriptor contract')
    sources = [item for item in device.board.devices if type(item).__name__ == 'AfsFrameSource']
    if len(sources) != 1 or not callable(getattr(sources[0], 'still_initialization_summary', None)):
        raise UnsupportedDevice('Still numeric admission awaits the actual S2 source and descriptor contract')
    clock = sources[0].still_initialization_summary()
    now = device.board.virtual_ticks
    if (type(clock) is not dict or clock.get('profile') != CLOCK_PROFILE
            or tuple(clock.get(key) for key in ('generation', 'cpu', 'source', 'physical_irq', 'state'))
                != (9, 0, 1, '0x29', 'waiting')
            or type(clock.get('requested_tick')) is not int or type(clock.get('due_tick')) is not int
            or not clock['requested_tick'] <= now < clock['due_tick']):
        raise UnsupportedDevice('Still numeric requires its future generation9 Source1 initialization clock')
    capture.validate(device)
    prior = device.rearm[capture.KEY]
    if (prior.get('retirement', {}).get('state') != 'completed'
            or prior.get('allocation_release', {}).get('state') != 'completed'
            or prior.get('data', {}).get('sample') != 2 or prior['data'].get('state') != 'acknowledged'
            or device.stop_profile != 'stock-idle-stop-handshakes-unmeasured' or not device._idle()
            or device.registers[device.GLOBAL] != 0x10030000
            or any(bank.enabled & (1 << 46) or bank.pending & (1 << 46)
                   or bank.active == 46 or bank.injected == 46
                   or any(entry.get('line') == 46 for entry in bank.active_stack)
                   for bank in device.board.interrupts.banks)):
        raise UnsupportedDevice('Still numeric requires the actual closed capture metering owner and retired IRQ46')
    caller = _caller(device)
    words = caller['words']
    expected = {0x15e32d0: 12, 0x15d502c: 5, 0x15d5084: 2, 0x15d5088: 0,
        0x15d4f64: 0x125ae3c, 0x15d4f68: 0x125adf8, 0x15d4f6c: 0x125ad54,
        0x15d4f74: 0x125ad20, 0x15d4fcc: 0, 0x15d4fd8: 1,
        0x15d4ff4: 100, 0x15d4ff6: 16, 0x15d5016: 3, 0x15d5017: 0,
        0x15d5019: 0, 0x15d501a: 0, 0x15d501c: 0,
        0x15f63a4: 2, 0x15f63a8: 0, 0x15f64a4: 0, 0x15f68a4: 0, 0x15f68a8: 0,
        0x15f6bf4: 0, 0x15f6e50: 1, 0x15f6e5c: 0, 0x15f6e60: 0,
        0x173d308: 0x13, 0x17d54dc: 1, 0x17d54e0: 0, 0x17d54ec: 0}
    descriptor = caller['descriptor']
    if (any(words[address] != value for address, value in expected.items())
            or not words[0x15d5044] or descriptor[0] != descriptor[4]
            or descriptor[1] != 0x06700670 or descriptor[2] != 0x09a409a4
            or descriptor[3] != 0x2700 or descriptor[7] != 0x2700
            or not 0x40000000 <= descriptor[0] < descriptor[0] + 9984 * 3296 <= 0x60000000
            or caller['table_header'] != 1
            or caller['table_rows'] != [[0x450, 0x2b8, 0xf0, 0xf0], *([[0, 0, 0, 0]] * 9)]):
        raise UnsupportedDevice('Still numeric descriptor, table or real still-state5 owner differs from the observed S2 caller')
    source = {key: deepcopy(value) for key, value in sources[0].request.items() if key != CLOCK_KEY}
    terminal = dict(registers=dict(device.registers), capture_sha256=hp.digest(prior), source=source)
    record = _record(device, terminal, clock, caller)
    device.rearm[KEY] = record
    device.board.record('Still numeric producer explicitly admitted', profile=profile,
        limits='Initializer only; no source event, DMA, IRQ, callback, exposure or capture result supplied')
    return deepcopy(record)


CALLER_FIELDS = ((0x15e32d0, 4), (0x15d502c, 4), (0x15d5044, 4), (0x15d5084, 4), (0x15d5088, 4),
    (0x15d4f64, 4), (0x15d4f68, 4), (0x15d4f6c, 4), (0x15d4f74, 4), (0x15d4fcc, 4), (0x15d4fd8, 4),
    (0x15d4ff4, 2), (0x15d4ff6, 2), (0x15d5016, 1), (0x15d5017, 1), (0x15d5019, 1), (0x15d501a, 1),
    (0x15d501c, 4), (0x15f63a4, 4), (0x15f63a8, 4), (0x15f64a4, 4), (0x15f68a4, 4), (0x15f68a8, 4),
    (0x15f6bf4, 4), (0x15f6e50, 4), (0x15f6e5c, 4), (0x15f6e60, 4), (0x173d308, 4),
    (0x17d54dc, 4), (0x17d54e0, 4), (0x17d54ec, 4))


def _caller(device):
    read = lambda address, size=4: int.from_bytes(device.board.read_memory(address, size), 'little')
    table = read(0x15d4fdc)
    if table & 3 or not 0 < table < table + 10 * 0x6c <= 0x20000000:
        raise UnsupportedDevice('Still numeric geometry table is outside aligned DRAM')
    return dict(words={address: read(address, size) for address, size in CALLER_FIELDS},
        descriptor=list(struct.unpack('<12I', device.board.read_memory(0x15d4f88, 48))),
        table=table, table_header=read(table, 1),
        table_rows=[list(struct.unpack('<4I', device.board.read_memory(table + index * 0x6c + 4, 16)))
                    for index in range(10)])


def historical_capture_view(device):
    """Restore the sealed predecessor for retained capture/AF consumers."""
    record = _owner(device)
    if record is None:
        return device
    terminal = record.get('terminal')
    if (type(terminal) is not dict
            or set(terminal) != {'registers', 'capture_sha256', 'source'}
            or type(terminal['registers']) is not dict or type(terminal['source']) is not dict
            or record.get('terminal_sha256') != hp.digest(terminal)
            or terminal['capture_sha256'] != hp.digest(device.rearm.get(capture.KEY))):
        raise UnsupportedDevice('Still numeric predecessor capture history changed')
    view = copy(device)
    view.registers = dict(terminal['registers'])
    view.rearm = {key: value for key, value in device.rearm.items() if key != KEY}
    view.board = copy(device.board)
    view.board.interrupts = copy(device.board.interrupts)
    view.board.interrupts.banks = [copy(bank) for bank in device.board.interrupts.banks]
    view.board.devices = []
    sources = 0
    for item in device.board.devices:
        proxy = view if item is device else copy(item)
        proxy.board = view.board
        if type(item).__name__ == 'AfsFrameSource':
            sources += 1
            proxy.request = deepcopy(terminal['source'])
        view.board.devices.append(proxy)
    if sources != 1:
        raise UnsupportedDevice('Still numeric predecessor requires its one retained source history')
    # The predecessor had genuinely retired IRQ46 before this registrar.
    # Its historical consumers must not inherit this new route/IRQ state.
    for bank in view.board.interrupts.banks:
        bank.enabled &= ~(1 << 46)
        bank.pending &= ~(1 << 46)
        if bank.active == 46:
            bank.active = None
        if bank.injected == 46:
            bank.injected = None
        bank.active_stack = [entry for entry in bank.active_stack if entry.get('line') != 46]
    return view


def _history(device):
    capture.validate(historical_capture_view(device))


def validate(device, *, check_history=True):
    record = _owner(device)
    if record is None:
        return None
    from . import afs_still_numeric as numeric
    if (type(record) is not dict
            or set(record) - {'data'} != {'profile', 'cpu', 'armed_tick', 'terminal', 'terminal_sha256',
                               'clock', 'clock_sha256', 'caller', 'caller_sha256', 'state', 'accesses'}
            or record['profile'] != PROFILE or type(record['cpu']) is not int or record['cpu'] != 0
            or type(record['armed_tick']) is not int
            or not 0 <= record['armed_tick'] <= device.board.virtual_ticks
            or type(record['state']) is not dict or set(record['state']) != af.STATE_KEYS
            or {**record['state']['words'], **numeric.words(device)} != device.registers or not af._idle(device)
            or type(record['accesses']) is not list
            or len(record['accesses']) > af.ACCESS_LIMIT + af.RESET_POLL_LIMIT):
        raise UnsupportedDevice('Still numeric plain owner/words or transaction budget changed')
    if not check_history:
        return record
    _history(device)
    if record['clock_sha256'] != hp.digest(record['clock']) or record['caller_sha256'] != hp.digest(record['caller']):
        raise UnsupportedDevice('Still numeric latched Source1 clock or caller identity changed')
    state = af._state(record['terminal']['registers'])
    previous = record['armed_tick']
    for event in record['accesses']:
        if type(event) is not dict or type(event.get('tick')) is not int or event['tick'] < previous:
            raise UnsupportedDevice('Still numeric transaction chronology changed')
        af._transition(device, state, event)
        previous = event['tick']
    state['request_route'] = record['state']['request_route']
    if state != record['state']:
        raise UnsupportedDevice('Still numeric hardware differs from its whole transaction replay')
    numeric.validate(device)
    _route(device, state)
    return record


def _route(device, state):
    if _owner(device).get('data') is not None:
        from . import afs_still_numeric as numeric
        route = numeric.route(device)
        expected = dict(cpu=0, vector=0x92c8cc, priority=13, source_configuration=1)
        if state['request_route'] != expected:
            raise UnsupportedDevice('Still numeric physical completion lost its retained initializer route')
        return route
    step = state['request_step']
    route = device._preview_request_route(0, step >= 16)
    if (step < 15 and route is not None
            or state['request_route'] != (route if step >= 16 else None)):
        raise UnsupportedDevice('Still numeric IRQ46 registration order changed')
    return route


def access(device, operation, cpu, address, size, value=None):
    record = validate(device, check_history=False)
    if record is None:
        return False
    if type(size) is not int or size != 4:
        raise UnsupportedDevice('Still numeric only admits CPU0 WORD transactions')
    state = record['state']
    _route(device, state)
    from . import afs_still_numeric as numeric
    if numeric.access(device, operation, cpu, address, size, value):
        return True
    route = (device._preview_request_route(0, state['request_step'] >= 15)
             if state['phase'] == 'activation' else state['request_route'])
    old = device.registers.get(address)
    event = dict(operation=operation, cpu=cpu, address=address,
                 word=old if operation == 'read' else value,
                 old_word=old, tick=device.board.virtual_ticks)
    projection = {**state, 'words': dict(state['words'])}
    af._transition(device, projection, event)
    if projection['request_step'] >= 16:
        projection['request_route'] = route
    if len(record['accesses']) >= af.ACCESS_LIMIT + af.RESET_POLL_LIMIT:
        raise UnsupportedDevice('Still numeric bounded transaction exhausted')
    record['accesses'].append(event)
    record['state'] = projection
    if operation == 'write':
        device.registers[address] = value
        device.writes.append(dict(cpu=cpu, address=hex(address), value=hex(value), old=hex(old)))
    return True


def tick(device, board):
    record = _owner(device)
    if record is None:
        return False
    from . import afs_still_numeric as numeric
    if numeric.tick(device, board): return True
    if record['state']['phase'] != 'resetting': return False
    if board.virtual_ticks < record['state']['reset_due_tick']:
        return False
    validate(device)
    event = dict(operation='response', cpu=None, address=device.GLOBAL,
                 word=0x10000000, old_word=device.registers[device.GLOBAL], tick=board.virtual_ticks)
    projection = {**record['state'], 'words': dict(record['state']['words'])}
    af._transition(device, projection, event)
    record['accesses'].append(event)
    record['state'] = projection
    device.registers[device.GLOBAL] = 0x10000000
    return True


def due(device):
    record = _owner(device)
    from . import afs_still_numeric as numeric
    output_due = numeric.due(device)
    if output_due is not None: return output_due
    return (record['state']['reset_due_tick']
            if record is not None and record['state']['phase'] == 'resetting' else None)


def source_anchor(device):
    """Pure retained initializer identity, without Source/history recursion."""
    record = _owner(device)
    if record is None or record['state']['request_step'] != 22:
        raise UnsupportedDevice('Still numeric clock requires its complete actual initializer')
    enabled = record['state']['request_enabled_tick']
    endings = [index for index, event in enumerate(record['accesses'])
        if (event.get('operation'), event.get('address'), event.get('word'), event.get('tick')) ==
           ('write', device.BASE + 8, 0x10000, enabled)]
    if (len(endings) != 1 or record['terminal_sha256'] != hp.digest(record['terminal'])
            or record['clock_sha256'] != hp.digest(record['clock'])
            or record['caller_sha256'] != hp.digest(record['caller'])):
        raise UnsupportedDevice('Still numeric initializer lost its sealed original caller or enable')
    state = af._state(record['terminal']['registers'])
    previous = record['armed_tick']
    prefix = record['accesses'][:endings[0] + 1]
    for event in prefix:
        if event['tick'] < previous:
            raise UnsupportedDevice('Still numeric initializer chronology changed')
        af._transition(device, state, event)
        previous = event['tick']
    route = dict(cpu=0, vector=0x92c8cc, priority=13, source_configuration=1)
    if (state['request_step'] != 22 or state['request_enabled_tick'] != enabled
            or record['state']['request_route'] != route):
        raise UnsupportedDevice('Still numeric initializer descriptor, enable or original IRQ46 changed')
    return dict(enabled_tick=enabled, sha256=hp.digest(dict(
        terminal_sha256=record['terminal_sha256'], clock_sha256=record['clock_sha256'],
        caller_sha256=record['caller_sha256'], accesses=prefix, route=route)))


def summary(device):
    record = validate(device)
    if record is None:
        return None
    state = record['state']
    return dict(profile=PROFILE, phase=state['phase'], accesses=len(record['accesses']),
                idle_step=state['idle_step'], request_step=state['request_step'],
                enabled_tick=state['request_enabled_tick'], route=state['request_route'],
                due_tick=due(device), limits='Initializer only; source clocks, RAW and numeric transport require separate physical owners')
