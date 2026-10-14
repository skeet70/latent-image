"""Explicit, flat, at-most-three JPEG lifetimes over stock91F620 reset.

Only closed outputs are retained. Mutable later descriptor/table RAM is not
historical evidence; the original latched bytes and programming ledger are.
Stock reset and IRQ unregister release live descriptor/table/output RAM;
the accepted programming ledger and output hash remain sealed evidence.
"""
from copy import copy, deepcopy

from . import jpeg_declared as declared


PROFILE = 'declared-channel8-distinct-context-successor-unmeasured'
MAX_LIFETIMES = 3


def _base(record):
    return {key: deepcopy(value) for key, value in record.items()
            if key not in ('history', 'successor')}


def _drained(device):
    irq = getattr(device.board, 'interrupts', None)
    if irq is None or len(irq.banks) != 2:
        declared._reject('successor requires the real two-bank interrupt controller')
    line = 77
    if any(bank.enabled & (1 << line) or bank.pending & (1 << line)
            or bank.active == line or bank.injected == line
            or any(item['line'] == line for item in bank.active_stack) for bank in irq.banks):
        declared._reject('successor requires disabled and drained IRQ77')
    read = lambda address: int.from_bytes(device.board.read_memory(address, 4, cpu_id=0), 'little')
    if read(0x15231a4) or read(0xc96f20 + line * 4):
        declared._reject('successor requires the real IRQ77 unregister')


def _entry(device):
    value = dict(record=_base(device.declared), registers=dict(device.registers),
                 closed_tick=device.board.virtual_ticks)
    return value | {'sha256': declared._seal(value)}


def _validate_entry(device, entry):
    if (not isinstance(entry, dict) or set(entry) != {'record', 'registers', 'closed_tick', 'sha256'}
            or declared._seal({key: value for key, value in entry.items() if key != 'sha256'})
                != entry['sha256']
            or type(entry['closed_tick']) is not int
            or not 0 <= entry['closed_tick'] <= device.board.virtual_ticks
            or not isinstance(entry['record'], dict) or entry['record'].get('state') != 'closed'
            or not isinstance(entry['registers'], dict)):
        declared._reject('closed JPEG history seal/chronology changed')
    view = copy(device)
    view.declared, view.registers = entry['record'], entry['registers']
    declared.validate(view, historical=True)
    if entry['closed_tick'] < entry['record']['close_requested_tick']:
        declared._reject('closed JPEG history predates its real cleanup request')


def validate(device):
    record = device.declared
    history = record.get('history', [])
    if not isinstance(history, list) or len(history) > MAX_LIFETIMES - 1:
        declared._reject('JPEG history exceeds three distinct lifetimes')
    seen, previous = set(), None
    for entry in history:
        _validate_entry(device, entry)
        latch = entry['record']['latch']
        if (latch['descriptor_address'] in seen
                or previous is not None and latch['armed_tick'] < previous):
            declared._reject('closed JPEG contexts/chronology are not distinct')
        seen.add(latch['descriptor_address'])
        previous = entry['closed_tick']
    if record['latch']['descriptor_address'] in seen:
        declared._reject('current JPEG descriptor repeats a retired context')
    if previous is not None and record['latch']['armed_tick'] < previous:
        declared._reject('current JPEG predates its retained history')
    marker = record.get('successor')
    if marker is not None:
        if (not isinstance(marker, dict) or set(marker) != {'profile', 'requested_tick',
                'state', 'events', 'previous'} or marker['profile'] != PROFILE
                or type(marker['requested_tick']) is not int
                or not record['close_requested_tick'] <= marker['requested_tick'] <= device.board.virtual_ticks
                or marker['state'] not in ('awaiting-reset', 'idle-requested', 'idle-acknowledged',
                                          'reset-requested', 'ready')
                or record['state'] != 'closed'):
            declared._reject('JPEG successor initialization marker changed')
        _validate_entry(device, marker['previous'])
        if marker['previous']['record'] != _base(record):
            declared._reject('JPEG successor no longer owns the preceding closure')
        _events(marker)
        if marker['events'] and marker['events'][-1]['tick'] > device.board.virtual_ticks:
            declared._reject('JPEG successor reset history is in the future')
        _current_words(device, marker)
    return history


def validate_initialization(device, record):
    value = record.get('initialization')
    if value is None:
        return
    if (not isinstance(value, dict) or set(value) != {'profile', 'requested_tick', 'events',
            'ready_tick', 'sha256'} or value['profile'] != PROFILE
            or declared._seal({key: item for key, item in value.items() if key != 'sha256'}) != value['sha256']
            or type(value['ready_tick']) is not int or type(value['requested_tick']) is not int
            or not value['requested_tick'] <= value['ready_tick'] <= record['latch']['armed_tick']):
        declared._reject('JPEG fresh initialization seal/chronology changed')
    _events(dict(state='ready', requested_tick=value['requested_tick'], events=value['events']))
    if not value['events'] or value['ready_tick'] != value['events'][-1]['tick']:
        declared._reject('JPEG fresh initialization lacks its final real control poll')


def _events(marker):
    stage, last = 'awaiting-reset', marker['requested_tick']
    events = marker['events']
    if not isinstance(events, list) or len(events) > 100000:
        declared._reject('JPEG successor reset is not bounded')
    for event in events:
        if (not isinstance(event, dict) or set(event) != {'operation', 'offset', 'value', 'tick'}
                or type(event['tick']) is not int or event['tick'] < last
                or type(event['value']) is not int):
            declared._reject('JPEG successor reset history changed')
        stage = _advance(stage, event['operation'], event['offset'], event['value'])
        last = event['tick']
    if stage != marker['state']:
        declared._reject('JPEG successor reset phase differs from its whole transaction')


def _advance(stage, operation, offset, value):
    if stage == 'awaiting-reset' and (operation, offset, value) == ('write', 0x118, 1):
        return 'idle-requested'
    if stage == 'idle-requested':
        if operation == 'read' and offset == 0x24 and value in (0, 0x10000):
            return stage
        if (operation, offset, value) == ('write', 0x24, 0x10000):
            return 'idle-acknowledged'
    if stage == 'idle-acknowledged' and (operation, offset, value) == ('write', 0, 0x100):
        return 'reset-requested'
    if stage == 'reset-requested' and operation == 'read' and offset == 0 and value in (0x100, 0):
        return 'ready' if value == 0 else stage
    declared._reject('successor initialization differs from whole91F620')


def arm_initialization(device, profile):
    if (profile != PROFILE or device.declared is None or device.declared['state'] != 'closed'
            or 'successor' in device.declared or device.state != 'idle' or device.pending is not None
            or device.declared['pending_action'] is not None
            or device.profile != 'declared-channel8-idle-reset-unmeasured'
            or getattr(device.board, 'failure', None) is not None):
        declared._reject('successor requires explicit admission after a completed JPEG closure')
    if len(validate(device)) >= MAX_LIFETIMES - 1:
        declared._reject('three JPEG lifetimes are already closed')
    # A terminal job's tables can already have been reused by the stock builder.
    # Its accepted protocol remains strictly validated after RAM retirement.
    declared.validate(device, historical=True)
    _drained(device)
    marker = dict(profile=profile, requested_tick=device.board.virtual_ticks,
                  state='awaiting-reset', events=[], previous=_entry(device))
    device.declared['successor'] = marker
    return dict(profile=profile, state=marker['state'], lifetime=len(device.declared.get('history', [])) + 2)


def access(device, operation, offset, value):
    marker = device.declared.get('successor') if device.declared is not None else None
    if marker is None:
        return False
    if offset not in (0, 0x24, 0x118):
        declared._reject('JPEG successor must finish its reset before parameter upload')
    _current_words(device, marker)
    if operation == 'write':
        expected_state = {0x118: 'idle', 0x24: 'idle-ready', 0: 'idle-acknowledged'}[offset]
        if device.pending is not None or device.state != expected_state:
            declared._reject('successor reset write precedes its actual idle/reset effect')
        validate(device)
    elif offset == 0x24 and (device.state not in ('idle-requested', 'idle-ready')
            or value != (0x10000 if device.state == 'idle-ready' else 0)):
        declared._reject('successor idle poll differs from current hardware phase')
    elif offset == 0 and (device.state not in ('reset-requested', 'idle')
            or value != (0x100 if device.state == 'reset-requested' else 0)):
        declared._reject('successor reset poll differs from current hardware phase')
    # Polls check the current phase; complete historical replay is performed at
    # admission, reset completion, summary/checkpoint and fresh upload admission.
    marker['state'] = _advance(marker['state'], operation, offset, value)
    marker['events'].append(dict(operation=operation, offset=offset, value=value,
                                 tick=device.board.virtual_ticks))
    return True


def _current_words(device, marker):
    """Cheap current word/owner checks for every bounded firmware poll."""
    expected = dict(marker['previous']['registers'])
    if device.state == 'idle' and marker['state'] in ('reset-requested', 'ready'):
        expected = dict.fromkeys(declared.OFFSETS, 0)
    elif device.state == 'idle-requested':
        expected[0x118] = 1
    elif device.state == 'idle-ready':
        expected[0x118], expected[0x24] = 1, 0x10000
    elif device.state == 'idle-acknowledged':
        expected[0x118], expected[0x24] = 1, 0
    elif device.state == 'reset-requested':
        expected[0x118], expected[0x24], expected[0] = 1, 0, 0x100
    elif device.state != 'idle':
        declared._reject('successor hardware phase is unknown')
    if device.registers != expected:
        declared._reject('successor current hardware words differ from its reset phase')


def before_reset(device):
    if device.declared is not None and 'successor' in device.declared:
        validate(device)
        marker = device.declared['successor']
        if marker['state'] != 'reset-requested':
            declared._reject('successor reset effect lacks its real reset request')


def completed_reset(device):
    if device.declared is not None and 'successor' in device.declared:
        # The next read of control0 completes the stock polling transaction.
        device.registers = dict.fromkeys(declared.OFFSETS, 0)
        validate(device)


def prepare_upload(device, descriptor_address):
    if device.declared is None:
        declared._reject('fresh upload lacks an admitted JPEG successor')
    history = validate(device)
    marker = device.declared.get('successor')
    if (marker is None or marker['state'] != 'ready' or device.state != 'idle'
            or device.pending is not None or any(device.registers.values())):
        declared._reject('fresh upload requires a complete successor91F620 reset')
    entries = history + [marker['previous']]
    if descriptor_address in {entry['record']['latch']['descriptor_address'] for entry in entries}:
        declared._reject('successor must use a distinct stock JPEG descriptor context')
    initialization = dict(profile=PROFILE, requested_tick=marker['requested_tick'],
        events=deepcopy(marker['events']), ready_tick=marker['events'][-1]['tick'])
    initialization['sha256'] = declared._seal(initialization)
    return deepcopy(entries), initialization


def check_destination(history, destination, capacity):
    start = declared._physical(destination, capacity)
    for entry in history:
        old = entry['record']
        address = declared._physical(old['latch']['destination'], old['output_bytes'])
        if start < address + old['output_bytes'] and address < start + capacity:
            declared._reject('successor destination overlaps a still-owned JPEG output')
