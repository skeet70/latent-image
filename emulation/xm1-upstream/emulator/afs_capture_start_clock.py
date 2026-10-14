"""One declared acquisition boundary before the first stock metering result.

The clock supplies physical IRQ27 only. Stock source0 subscribers own
bank rotation and3F01; stock12FA7E8 moves private state4 to5.
The pre-edge descriptor belongs to the first BankA result even though the
edge programs BankB. No numeric output, callback or guest write is supplied.
"""
from copy import deepcopy

from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-afs-generation8-metering-acquisition-start-source0-clock-unmeasured'
KEY = 'acquisition_start'
PARENT = 'capture_initialization'
IRQ = 0x27
WORDS = ((0x15e32d0, 4, 11), (0x15d4908, 4, 4), (0x15d490c, 4, 1),
         (0x173d308, 4, 0x13), (0x15f63a4, 4, 3), (0x15f68a4, 4, 1),
         (0x15f68a8, 4, 0x12f604c), (0x15f6bf4, 4, 1),
         (0x15d4888, 1, 0), (0x15d488c, 4, 0x1258d08),
         (0x15d4890, 4, 0x1258cdc), (0x15d4dbc, 1, 0),
         (0x1797284, 1, 0), (0x15f8814, 2, 3),
         (0x15f6e44, 4, 1), (0x15f6e40, 4, 1), (0x15f6e3c, 4, 1),
         (0x15f6bf8, 4, 0x5fecaa60))
FIELDS = {'profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector',
          'requested_tick', 'due_tick', 'cadence_ticks', 'contract', 'initialization_sha256',
          'input_sha256', 'state_sha256', 'state', 'asserted_tick', 'acknowledged_tick',
          'eoi_tick', 'write_cursor', 'assertion_write_index', 'ack_write_index'}
IDENTITY = ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector',
            'requested_tick', 'due_tick', 'cadence_ticks', 'contract', 'initialization_sha256')


def _u(source, address, size=4):
    raw = bytes(source.board.read_memory(address, size, cpu_id=0))
    if len(raw) != size:
        raise UnsupportedDevice('Metering acquisition-start clock caller RAM is truncated')
    return int.from_bytes(raw, 'little')


def _marker(source):
    parent = source.request.get(PARENT) if type(source.request) is dict else None
    return parent.get(KEY) if type(parent) is dict else None


def _initialization(source):
    parent = source.request.get(PARENT) if type(source.request) is dict else None
    if (type(parent) is not dict or parent.get('state') != 'acknowledged'
            or parent.get('eoi_tick') is None):
        raise UnsupportedDevice('Acquisition-start requires the closed actual initialization clock')
    return {key: value for key, value in parent.items() if key not in ('acquisition_start', 'finite_stop')}


def _producer(source):
    producer = source._device('ProducerControl')
    getter = getattr(producer, 'capture_metstats_source_anchor', None)
    if not callable(getter):
        raise UnsupportedDevice('Capture acquisition clock lacks its qualified producer activation')
    anchor = getter()
    if (type(anchor) is not dict or set(anchor) != {'enabled_tick', 'sha256'}
            or type(anchor['enabled_tick']) is not int or not 0 <= anchor['enabled_tick'] < source.board.virtual_ticks
            or type(anchor['sha256']) is not str or len(anchor['sha256']) != 64):
        raise UnsupportedDevice('Capture acquisition producer activation is not a complete retained identity')
    return deepcopy(anchor)


def _no_data(source):
    producer = source._device('ProducerControl')
    owner = producer.rearm.get('capture_metstats') if type(producer.rearm) is dict else None
    if (type(owner) is not dict or owner.get('data') is not None
            or owner.get('data_history') not in (None, []) or owner.get('retirement') is not None):
        raise UnsupportedDevice('Capture acquisition clock must precede all capture numeric data')
    return producer


def _seal(record):
    record['state_sha256'] = digest({key: value for key, value in record.items() if key != 'state_sha256'})


def _drained(source):
    return all(not bank.pending & (1 << IRQ) and bank.active != IRQ and bank.injected != IRQ
               for bank in source.board.interrupts.banks)


def _route(source, inputs):
    bank = source.board.interrupts.banks[0]
    if (inputs._event_route(IRQ) != 0 or not _drained(source)
            or bank.config.get(0x400 + 4 * IRQ) != 0x9df0b8
            or bank.config.get(0x800 + 4 * IRQ) != 10
            or not bank.source_config & (1 << IRQ)):
        raise UnsupportedDevice('Metering acquisition-start clock lacks its drained CPU0 source0 route')
    return {'cpu': 0, 'irq': IRQ, 'vector': 0x9df0b8, 'priority': 10, 'source_type': 1}


def _live(source):
    inputs = source._prefix(historical=True)
    if (inputs.event_enable_generation != 8 or inputs.registers.get(0x38c) != 0x10201
            or inputs.registers.get(0x48) != 1 or inputs.registers.get(0x4c) != 1
            or inputs.registers.get(0x70) != 0 or inputs.registers.get(0x154) != 0
            or inputs.busy or inputs.phase_busy or inputs.pending_command is not None
            or inputs.command_armed is not None):
        raise UnsupportedDevice('Metering acquisition-start clock input gates differ from the idle generation8 owner')
    if any(_u(source, address, size) != value for address, size, value in WORDS):
        raise UnsupportedDevice('Metering acquisition-start clock belongs to another sensor/private capture/lower18 owner')
    if _u(source, 0x15f6e50) != 1:
        raise UnsupportedDevice('Capture metering source0 clock requires its actual DMA owner1')
    base = 0xc6dc58
    tokens = [_u(source, base + 4 + 4 * index) for index in range(2)]
    if (_u(source, base, 1) != 2 or _u(source, base + 64, 1) != 0
            or any(not token for token in tokens) or len(set(tokens)) != 2
            or tokens != [_u(source, 0x15f6e5c), _u(source, 0x15d4910)]
            or [(_u(source, base + 24 + 8 * index), _u(source, base + 28 + 8 * index))
                for index in range(2)] != [(0x11f6008, 2), (0x12fa47c, 1)]):
        raise UnsupportedDevice('Metering acquisition-start requires both active stock source0 subscribers')
    if any(bank.pending & (1 << 46) or bank.active == 46 or bank.injected == 46
           for bank in source.board.interrupts.banks):
        raise UnsupportedDevice('Metering acquisition-start requires a drained pre-data numeric IRQ46 route')
    producer = _no_data(source)
    activation = _producer(source)
    destinations = {address: producer.registers[address] for address in producer.DESTINATIONS}
    offsets = (0, 0x800, 0x850, 0x1850, 0x1990, 0x1d90, 0x2190, 0x21b0, 0x23b0)
    if tuple(destinations.values()) != tuple(0x5fecaa60 + offset for offset in offsets):
        raise UnsupportedDevice('Capture acquisition clock requires the actual initial BankA descriptor')
    callbacks = [{'source': 0, 'tokens': tokens, 'callbacks': [[0x11f6008, 2], [0x12fa47c, 1]]},
                 {'source': 1, 'tokens': [], 'callbacks': []}]
    return inputs, {'generation': 8, 'mask': 0x10201, 'words': [list(word) for word in WORDS],
                    'producer': activation, 'destinations': destinations, 'bank': 0x5fecaa60, 'callbacks': callbacks,
                    'route': _route(source, inputs), 'prefix_sha256': digest(inputs.event_history)}


def arm(source, profile, *, cadence_ticks=1000):
    if (profile != PROFILE or source.board.failure is not None or type(source.request) is not dict
            or _marker(source) is not None
            or type(cadence_ticks) is not int or not 1 <= cadence_ticks <= 400000):
        raise UnsupportedDevice('Metering acquisition-start clock requires its named one-edge profile and bounded delay')
    source._validate()
    initialization_sha = digest(_initialization(source))
    inputs, contract = _live(source)
    now = source.board.virtual_ticks
    if type(now) is not int or not 0 <= now < (1 << 64) - cadence_ticks:
        raise UnsupportedDevice('Metering acquisition-start clock virtual time is outside its integer range')
    record = dict(profile=profile, generation=8, cpu=0, source=0, physical_irq='0x27', ack_selector=1,
                  requested_tick=now, due_tick=now + cadence_ticks, cadence_ticks=cadence_ticks,
                  contract=contract, initialization_sha256=initialization_sha,
                  state='waiting', asserted_tick=None,
                  acknowledged_tick=None, eoi_tick=None, write_cursor=len(inputs.writes),
                  assertion_write_index=None, ack_write_index=None)
    record['input_sha256'] = digest({key: record[key] for key in IDENTITY})
    _seal(record)
    source.request[PARENT][KEY] = record
    source.board.record('Capture metering acquisition-start clock explicitly armed', profile=profile,
                        due_tick=record['due_tick'], limits='One source0 IRQ; no source1, image, result or callback')
    return deepcopy(record)


def validate(source):
    record = _marker(source)
    if record is None:
        return None
    if (type(record) is not dict or set(record) != FIELDS
            or tuple(record[key] for key in ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'ack_selector')) !=
               (PROFILE, 8, 0, 0, '0x27', 1)
            or any(type(record[key]) is not int for key in ('requested_tick', 'due_tick', 'cadence_ticks', 'write_cursor'))
            or not 1 <= record['cadence_ticks'] <= 400000
            or not 0 <= record['requested_tick'] < record['due_tick'] == record['requested_tick'] + record['cadence_ticks'] < 1 << 64
            or record['requested_tick'] > source.board.virtual_ticks
            or record['input_sha256'] != digest({key: record[key] for key in IDENTITY})
            or record['state_sha256'] != digest({key: value for key, value in record.items() if key != 'state_sha256'})
            or record['initialization_sha256'] != digest(_initialization(source))
            or record['state'] not in ('waiting', 'asserted', 'acknowledged')):
        raise UnsupportedDevice('Metering acquisition-start clock identity, history, timer or plain ledger changed')
    contract = record['contract']
    if (type(contract) is not dict
            or set(contract) != {'generation', 'mask', 'words', 'producer', 'destinations', 'bank', 'callbacks', 'route', 'prefix_sha256'}
            or contract['generation'] != 8 or contract['mask'] != 0x10201
            or contract['words'] != [list(word) for word in WORDS]
            or contract['route'] != {'cpu': 0, 'irq': IRQ, 'vector': 0x9df0b8, 'priority': 10, 'source_type': 1}
            or contract['bank'] != 0x5fecaa60
            or contract['producer'] != _producer(source)
            or type(contract['destinations']) is not dict
            or list(contract['destinations']) != list(source._device('ProducerControl').DESTINATIONS)
            or tuple(contract['destinations'].values()) != tuple(0x5fecaa60 + offset for offset in
                (0, 0x800, 0x850, 0x1850, 0x1990, 0x1d90, 0x2190, 0x21b0, 0x23b0))
            or type(contract['callbacks']) is not list or len(contract['callbacks']) != 2):
        raise UnsupportedDevice('Metering acquisition-start retained owner/route template changed')
    for number, expected in ((0, [[0x11f6008, 2], [0x12fa47c, 1]]), (1, [])):
        callbacks = contract['callbacks'][number]
        if (type(callbacks) is not dict or set(callbacks) != {'source', 'tokens', 'callbacks'}
                or callbacks['source'] != number or callbacks['callbacks'] != expected
                or type(callbacks['tokens']) is not list or len(callbacks['tokens']) != len(expected)
                or any(type(token) is not int or not 0 < token < 1 << 32 for token in callbacks['tokens'])):
            raise UnsupportedDevice('Metering acquisition-start retained subscriber template changed')
    tokens = contract['callbacks'][0]['tokens'] + contract['callbacks'][1]['tokens']
    if len(set(tokens)) != 2:
        raise UnsupportedDevice('Metering acquisition-start retained source handles are aliased')
    inputs = source._prefix(historical=True)
    if (record['contract'].get('prefix_sha256') != digest(inputs.event_history)
            or not 0 <= record['write_cursor'] <= len(inputs.writes)):
        raise UnsupportedDevice('Metering acquisition-start clock lost its original input history or ACK cursor')
    assertion, acknowledgment = record['assertion_write_index'], record['ack_write_index']
    if record['state'] in ('asserted', 'acknowledged'):
        if type(assertion) is not int or not 0 <= assertion <= record['write_cursor']:
            raise UnsupportedDevice('Metering acquisition-start physical assertion lost its input write boundary')
    elif assertion is not None:
        raise UnsupportedDevice('Metering acquisition-start waiting input claims an assertion write boundary')
    if record['state'] == 'acknowledged':
        if (type(acknowledgment) is not int or not assertion <= acknowledgment < record['write_cursor']
                or inputs.writes[acknowledgment]['cpu'] != 0
                or inputs.writes[acknowledgment]['address'] != hex(inputs.BASE + 0x388)
                or inputs.writes[acknowledgment]['value'] != '0x1'):
            raise UnsupportedDevice('Metering acquisition-start acknowledgment lost the actual CPU0 selector1 write')
    elif acknowledgment is not None:
        raise UnsupportedDevice('Metering acquisition-start input claims an acknowledgment without its physical boundary')
    previous = record['due_tick']
    for field in ('asserted_tick', 'acknowledged_tick', 'eoi_tick'):
        value = record[field]
        if value is not None:
            if type(value) is not int or previous is None or not previous <= value <= source.board.virtual_ticks:
                raise UnsupportedDevice('Metering acquisition-start clock physical assertion/ACK/EOI chronology changed')
        previous = value
    if ((record['state'] in ('asserted', 'acknowledged')) != (record['asserted_tick'] is not None)
            or (record['state'] == 'acknowledged') != (record['acknowledged_tick'] is not None)
            or record['state'] != 'acknowledged' and record['eoi_tick'] is not None):
        raise UnsupportedDevice('Metering acquisition-start clock claims an unobserved physical boundary')
    return record


def tick(source, board):
    record = _marker(source)
    if record is None or record.get('eoi_tick') is not None or board.failure is not None:
        return
    try:
        inputs = source._device('InputProcessingControl')
        if record['state'] == 'waiting':
            if board.virtual_ticks < record['due_tick']:
                return
            source._validate()
            _, live = _live(source)
            if live != record['contract']:
                raise UnsupportedDevice('Metering acquisition-start caller changed before its one physical clock')
            record.update(state='asserted', asserted_tick=board.virtual_ticks, write_cursor=len(inputs.writes),
                          assertion_write_index=len(inputs.writes))
            _seal(record)
            board.queue_irq(0, IRQ)
            board.record('Capture metering acquisition-start source0 asserted', tick=board.virtual_ticks,
                         limits='No source1, DMA, firmware message, callback or AF result')
        elif record['state'] == 'asserted':
            hits = [(index, item) for index, item in enumerate(inputs.writes[record['write_cursor']:], record['write_cursor'])
                    if item['address'] == hex(inputs.BASE + 0x388) and item['value'] == '0x1']
            if hits:
                validate(source)
                if any(item['cpu'] != 0 for _, item in hits):
                    raise UnsupportedDevice('Metering acquisition-start selector1 ACK came from another CPU')
                record.update(state='acknowledged', acknowledged_tick=board.virtual_ticks,
                              write_cursor=len(inputs.writes), ack_write_index=hits[0][0])
                _seal(record)
        if record['state'] == 'acknowledged' and record['eoi_tick'] is None and _drained(source):
            validate(source)
            record['eoi_tick'] = board.virtual_ticks
            _seal(record)
    except UnsupportedDevice as error:
        board.failure = dict(message=str(error), cpu=0, device='AfsFrameSource', operation='capture-acquisition-start-clock')


def due(source):
    record = _marker(source)
    return record['due_tick'] if record is not None and record.get('state') == 'waiting' else None


def summary(source):
    record = validate(source)
    return deepcopy(record)
