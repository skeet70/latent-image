"""Explicit contrast-AF frames from real source/pose pairs.

The desired output tuple and programmed windows are latched before Source0.
Scene bytes depend on the stock Source1 pose-array sample; they precede IRQ46
and its unchanged paired ACK driver. No metadata, callbacks or AF decisions
are supplied. The first profile admits one job. An explicit continuation
requires the previous paired ACK/EOI and contiguous stock sample counters.
Closed jobs remain a bounded flat history; no recursive archive is retained.
"""
from copy import deepcopy
import hashlib
import struct

from .hardware import UnsupportedDevice
from . import histogram_protocol as hp
from . import afs_focus_scene
from . import afs_statistics_frame as previous_statistics
from .virtual_source import ram_span


PROFILE = 'declared-afs-first-sampled-pose-contrast-data-unmeasured'
CONTINUATION_PROFILE = 'declared-afs-contiguous-sampled-pose-contrast-data-continuation-unmeasured'
MAX_PAIRS = 251
DATA_TICKS = 1024
ACK_OFFSETS = (0x88c, 0x8f4, 0x98c, 0x9f4)
EVENT_LIMIT = 4096


def _seal(data):
    data['state_sha256'] = hp.digest({key: value for key, value in data.items() if key != 'state_sha256'})


def _source(device, pair_id):
    from . import afs_frame_source
    matches = [item for item in afs_frame_source.af_requests(device.board)
               if item.get('kind') == 'pixel' and item.get('pair_id') == pair_id]
    if len(matches) != 1:
        raise UnsupportedDevice('AF numeric data requires its separate real pixel-pair identity')
    return matches[0]


def _pair(board, pair_id):
    from . import afs_frame_source
    return afs_frame_source.af_pair(board, pair_id)


def _owner(device):
    record = device.rearm.get('afs_focus') if type(device.rearm) is dict else None
    if (type(record) is not dict or record.get('state', {}).get('phase') != 'enabled-awaiting-input'
            or record['state'].get('request_step') != 22):
        raise UnsupportedDevice('AF numeric data requires the complete real contrast activation')
    return record


def _read(board, address, size):
    raw = bytes(board.read_memory(address, size))
    if len(raw) != size:
        raise UnsupportedDevice('AF numeric configuration is not mapped guest memory')
    return raw


def _configuration(device):
    board = device.board
    packet = _read(board, 0x16224d8, 24)
    width, height, x, y, gaps, counts = struct.unpack('<6I', packet)
    dimensions = _read(board, 0x16227a4, 6)
    if counts & ~0xffff or dimensions != bytes((counts & 255, counts >> 8)) * 3:
        raise UnsupportedDevice('AF numeric consumer dimensions differ from the programmed packet')
    windows = _read(board, 0x16228c0, 164)
    count = struct.unpack_from('<I', windows)[0]
    if count != 10:
        raise UnsupportedDevice('AF numeric profile qualifies the actual ten-region descriptor only')
    return dict(packet_hex=packet.hex(), dimensions_hex=dimensions.hex(),
        grid=dict(cell_width=width, cell_height=height, x=x, y=y, gap_x=gaps & 65535,
                  gap_y=gaps >> 16, columns=counts & 255, rows=counts >> 8),
        windows=[list(struct.unpack_from('<4I', windows, 4 + index * 16)) for index in range(10)],
        band_count=_read(board, 0x16227aa, 1)[0],
        band_scale=int.from_bytes(_read(board, 0x1622508, 4), 'little'),
        band_orientation=int.from_bytes(_read(board, 0x1622888, 4), 'little'),
        desired_words={address: device.registers[address] for address in device.KNOWN_DESIRED_CONFIG})


def arm(device, profile=PROFILE):
    from . import producer_afs_focus
    from . import afs_frame_source
    record = producer_afs_focus.validate(device)
    now = device.board.virtual_ticks
    marker = afs_frame_source.af_pending_pair(device.board)
    old_data = record.get('data') if record is not None else None
    history = record.get('data_history', []) if record is not None else []
    pair_id = 1 if old_data is None else old_data['latch']['pair_id'] + 1
    expected = PROFILE if pair_id == 1 else CONTINUATION_PROFILE
    if (profile != expected or record is None or record is not _owner(device)
            or marker is None or not 1 <= pair_id <= MAX_PAIRS
            or marker.get('kind') != 'pixel' or marker.get('pair_id') != pair_id
            or marker.get('phase_at_arm') != (12 if pair_id == 1 else 13)
            or marker.get('pose_index') != pair_id - 1
            or marker.get('source_count_at_arm') != pair_id - 1
            or marker.get('data_count_at_arm') != pair_id - 1
            or marker.get('producer') != producer_afs_focus.source_anchor(device)
            or marker.get('source0', {}).get('state') != 'waiting'
            or not record['state']['request_enabled_tick'] <= now < marker['source0']['due_tick']
            or type(device.ack_ticks) is not int or device.ack_ticks < 1):
        raise UnsupportedDevice('AF numeric job requires the next future contiguous state12/13 pixel pair')
    if old_data is not None:
        if old_data['state'] != 'acknowledged' or len(history) != pair_id - 2:
            raise UnsupportedDevice('AF continuation requires the previous paired data ACK')
        if (int.from_bytes(_read(device.board, 0x16223fc, 2), 'little') != pair_id - 1
                or int.from_bytes(_read(device.board, 0x16223fe, 2), 'little') != pair_id - 1
                or any(bank.pending & (1 << 46) or bank.active == 46 or bank.injected == 46
                       for bank in device.board.interrupts.banks)):
            raise UnsupportedDevice('AF continuation awaits real data EOI and stock counter progression')
    previous_statistics._route(device)
    scene, _ = previous_statistics._scene(device)
    measurement = device.rearm.get('measurement', {})
    old = measurement.get('data')
    if type(old) is not dict or old.get('state') != 'acknowledged':
        raise UnsupportedDevice('AF scene requires the retained initial virtual-scene target')
    target = old['latch']['optical']['target_position']
    if old_data is not None and target != old_data['latch']['target_position']:
        raise UnsupportedDevice('AF continuation cannot relatch the scene target to a new lens pose')
    configuration = _configuration(device)
    destinations = {address: device.registers[address] for address in device.DESTINATIONS}
    offsets = (0, 0x800, 0x850, 0x1850, 0x1990, 0x1d90, 0x2190, 0x21b0, 0x23b0)
    base = destinations[device.BASE + 0x500]
    if (base != (0x5fecaa60 if pair_id & 1 else 0x5fecd410)
            or tuple(destinations[address] - base for address in device.DESTINATIONS) != offsets):
        raise UnsupportedDevice('AF consumer spans require the actual contiguous bank descriptor')
    prior = {device.BASE + offset: device.registers[device.BASE + offset] for offset in ACK_OFFSETS}
    if tuple(prior.values()) != ((0, 1, 0, 0) if pair_id == 1 else (0, 0, 0, 0)):
        raise UnsupportedDevice('AF numeric data requires its setup or preceding paired ACK latches')
    latch = dict(producer=deepcopy(marker['producer']), pair_id=pair_id, input_sha256=marker['input_sha256'],
        scene=scene, target_position=target, destinations=destinations, configuration=configuration,
        data_ticks=DATA_TICKS, ack_ticks=device.ack_ticks)
    if old_data is not None:
        latch['previous_data_sha256'] = old_data['state_sha256']
    data = dict(profile=profile, state='waiting-pair', armed_tick=now, latch=latch, latch_sha256=hp.digest(latch),
        pair=None, result=None, due_tick=None, completed_tick=None, acknowledged_tick=None,
        prior_ack_words=prior, ack_words=dict(prior), ack_channel=0, ack_step=0, ack_due_tick=None, events=[], rotation=None)
    _seal(data)
    if old_data is not None:
        record.setdefault('data_history', []).append(deepcopy(old_data))
    record['data'] = data
    device.board.record('First contrast AF numeric frame explicitly armed', profile=profile,
        pair_id=pair_id, limits='Output tuple latched before Source0; scene awaits actual Source1 pose consumption')
    return deepcopy(data)


def _prepare(device, data, pair):
    pair_id = data['latch']['pair_id']
    if (pair.get('pair_id') != pair_id or pair.get('pose_index') != pair_id - 1
            or pair.get('input_sha256') != data['latch']['input_sha256']
            or pair.get('producer') != data['latch']['producer']
            or type(pair.get('sampled_focus')) is not int or type(pair.get('consumed_tick')) is not int
            or not data['armed_tick'] <= pair['consumed_tick'] <= device.board.virtual_ticks):
        raise UnsupportedDevice('AF pixel pair lost its real pose-array sample')
    if (type(data.get('rotation')) is not dict or data['rotation'].get('state') != 'completed'
            or len(data['rotation']['events']) != 9):
        raise UnsupportedDevice('AF numeric extraction requires its whole Source0 bank rotation')
    scene = data['latch']['scene']; config = data['latch']['configuration']
    image = _read(device.board, int(scene['address'], 16), scene['bytes'])
    if hashlib.sha256(image).hexdigest() != scene['payload_sha256']:
        raise UnsupportedDevice('AF frozen scene changed before pose-dependent extraction')
    result = afs_focus_scene.prepare(image, width=scene['width'], height=scene['height'], stride_bytes=scene['stride_bytes'],
        focus_position=pair['sampled_focus'], target_position=data['latch']['target_position'],
        dimensions=bytes.fromhex(config['dimensions_hex']),
        **{key: config[key] for key in ('grid', 'windows', 'band_count', 'band_scale', 'band_orientation')})
    base = data['latch']['destinations'][device.BASE + 0x500]
    plan = []; ranges = []
    for offset, raw in result.pop('spans'):
        address = base + offset; low, high = ram_span(address, len(raw))
        if address & 3 or any(low < end and start < high for start, end in ranges):
            raise UnsupportedDevice('AF numeric windows overlap or escape aligned DRAM')
        ranges.append((low, high)); plan.append(dict(address=address, offset=offset, bytes=len(raw), payload_hex=raw.hex()))
    result['plan'] = plan
    result['dimensions_hex'] = result.pop('dimensions').hex()
    result['contrast'].pop('bank_spans')
    due = pair['consumed_tick'] + DATA_TICKS
    if due + 2 * device.ack_ticks >= 1 << 64:
        raise UnsupportedDevice('AF data/ACK delay escapes the virtual clock')
    data.update(state='pending', pair=deepcopy(pair), result=result, due_tick=due)
    _seal(data)


def arm_next(device, profile=CONTINUATION_PROFILE):
    record = _owner(device)
    current = record.get('data')
    if (profile != CONTINUATION_PROFILE or type(current) is not dict
            or current.get('state') != 'acknowledged'):
        raise UnsupportedDevice('AF continuation requires an explicit profile and a closed current job')
    return arm(device, profile)


def _validate_data(device, data, *, check_source=True, check_route=False, source_view=None, activation=None):
    from . import producer_afs_focus
    if data is None: return None
    pair_id = data.get('latch', {}).get('pair_id') if type(data) is dict else None
    if (type(data) is not dict or type(pair_id) is not int or not 1 <= pair_id <= MAX_PAIRS
            or data.get('profile') != (PROFILE if pair_id == 1 else CONTINUATION_PROFILE)
            or data.get('state') not in ('waiting-pair', 'pending', 'delivered', 'acknowledged')
            or data.get('state_sha256') != hp.digest({key: value for key, value in data.items() if key != 'state_sha256'})
            or data.get('latch_sha256') != hp.digest(data['latch'])
            or data['latch']['producer'] != (activation if activation is not None else producer_afs_focus.source_anchor(device))
            or data['latch']['data_ticks'] != DATA_TICKS
            or data['latch']['ack_ticks'] != device.ack_ticks):
        raise UnsupportedDevice('AF numeric job seal/activation changed')
    if check_source:
        marker = source_view[pair_id] if source_view is not None else _source(device, pair_id)
        if marker.get('input_sha256') != data['latch']['input_sha256'] or marker.get('producer') != data['latch']['producer']:
            raise UnsupportedDevice('AF numeric job lost its immutable source identity')
        if data['pair'] is not None:
            from .afs_contrast_clock import pair_projection
            actual = pair_projection(marker) if source_view is not None else _pair(device.board, pair_id)
            if actual != data['pair']:
                raise UnsupportedDevice('AF numeric job pose sample changed')
    if data['state'] == 'waiting-pair':
        if any(data[key] is not None for key in ('pair', 'result', 'due_tick', 'completed_tick')):
            raise UnsupportedDevice('AF numeric job claims data before the real pose sample')
    elif (type(data['pair']) is not dict or type(data['result']) is not dict
            or data['due_tick'] != data['pair']['consumed_tick'] + DATA_TICKS):
        raise UnsupportedDevice('AF numeric result lacks its actual pose/data delay')
    delivered = data['state'] in ('delivered', 'acknowledged')
    if (delivered and (type(data['completed_tick']) is not int
            or not data['due_tick'] <= data['completed_tick'] <= device.board.virtual_ticks)
            or not delivered and data['completed_tick'] is not None):
        raise UnsupportedDevice('AF numeric DMA precedes its sample/completion delay')
    _validate_ack(device, data)
    _validate_rotation(device, data, check_source=check_source, source_view=source_view)
    if check_route: previous_statistics._route(device, pending=delivered)
    return data


def validate(device, *, check_source=True, check_route=False):
    record = _owner(device); data = record.get('data')
    history = record.get('data_history', [])
    if (type(history) is not list or len(history) >= MAX_PAIRS
            or history and data is None):
        raise UnsupportedDevice('AF data history must be a bounded flat closed list')
    # One consuming traversal resolves Source's flat stream once. It does
    # not copy/revalidate that whole stream separately for every old job.
    source_view = None
    if check_source and data is not None:
        from . import afs_frame_source
        source_view = {}
        for item in afs_frame_source.af_requests(device.board):
            if item.get('kind') != 'pixel': continue
            pair_id = item.get('pair_id')
            if type(pair_id) is not int or not 1 <= pair_id <= MAX_PAIRS or pair_id in source_view:
                raise UnsupportedDevice('AF data source view lost its bounded unique pair identities')
            source_view[pair_id] = item
    from . import producer_afs_focus
    activation = producer_afs_focus.source_anchor(device) if data is not None else None
    previous = None
    for index, item in enumerate((*history, data) if data is not None else (), 1):
        if (type(item) is not dict or item.get('latch', {}).get('pair_id') != index
                or index <= len(history) and item.get('state') != 'acknowledged'
                or item['latch'].get('previous_data_sha256') != previous):
            raise UnsupportedDevice('AF data history lost its contiguous immutable predecessor')
        if check_source or item is data:
            if check_source and index not in source_view:
                raise UnsupportedDevice('AF data history lost its physical source identity')
            _validate_data(device, item, check_source=check_source,
                           check_route=check_route and item is data, source_view=source_view, activation=activation)
        previous = item['state_sha256']
    return data


def _source0_epoch(marker):
    item = marker['source0']
    fields = ('profile', 'generation', 'ordinal', 'attempt', 'frame', 'cpu', 'source',
              'physical_irq', 'ack_selector', 'asserted_tick')
    if (type(item) is not dict or item.get('state') not in ('asserted', 'acknowledged')
            or type(item.get('asserted_tick')) is not int
            or (item.get('cpu'), item.get('source'), item.get('physical_irq'), item.get('ack_selector'))
                != (0, 0, '0x27', 1)):
        raise UnsupportedDevice('AF desired-bank rotation requires the actual Source0 assertion')
    return {key: deepcopy(item[key]) for key in fields}


def _validate_rotation(device, data, *, check_source=True, source_view=None):
    rotation = data.get('rotation')
    if rotation is None: return None
    if (type(rotation) is not dict or set(rotation) != {'source_epoch', 'events', 'state'}
            or type(rotation['events']) is not list or not 1 <= len(rotation['events']) <= 9
            or rotation['state'] != ('completed' if len(rotation['events']) == 9 else 'writing')
            or data['latch']['destinations'][device.BASE + 0x500] !=
               (0x5fecaa60 if data['latch']['pair_id'] & 1 else 0x5fecd410)):
        raise UnsupportedDevice('AF bank rotation requires its actual alternating-bank callback')
    if check_source:
        marker = source_view[data['latch']['pair_id']] if source_view is not None else _source(device, data['latch']['pair_id'])
        if rotation['source_epoch'] != _source0_epoch(marker):
            raise UnsupportedDevice('AF bank rotation lost its physical input epoch')
    previous = rotation['source_epoch']['asserted_tick']
    for event, address in zip(rotation['events'], device.REARM_ROTATION_ORDER):
        if (type(event) is not dict or set(event) != {'operation', 'cpu', 'address', 'word', 'old_word', 'tick'}
                or (event['operation'], event['cpu'], event['address'], event['word'], event['old_word'])
                     != ('rotation', 0, address, data['latch']['destinations'][address]
                          + (0x29b0 if data['latch']['pair_id'] & 1 else -0x29b0),
                         data['latch']['destinations'][address])
                or type(event['tick']) is not int or not previous <= event['tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('AF bank rotation differs from the whole nine-store driver')
        previous = event['tick']
    return rotation


def rotation_event(device, core, address, size, value):
    """Validate one actual driver store before the producer projects it."""
    data = validate(device, check_source=False)
    if (data is None or data['state'] not in ('waiting-pair', 'pending') or type(core) is not int
            or core != 0 or type(size) is not int or size != 4 or address not in device.DESTINATIONS):
        raise UnsupportedDevice('AF rotation requires its admitted pixel job and CPU0 WORD driver')
    marker = _source(device, data['latch']['pair_id']); epoch = _source0_epoch(marker)
    rotation = data['rotation']; step = len(rotation['events']) if rotation is not None else 0
    if (step >= 9 or address != device.REARM_ROTATION_ORDER[step]
            or value != data['latch']['destinations'][address]
               + (0x29b0 if data['latch']['pair_id'] & 1 else -0x29b0)
            or device.registers[address] != data['latch']['destinations'][address]):
        raise UnsupportedDevice('AF rotation requires its next alternating-bank pointer store')
    event = dict(operation='rotation', cpu=core, address=address, word=value,
                 old_word=device.registers[address], tick=device.board.virtual_ticks)
    if rotation is None: rotation = dict(source_epoch=epoch, events=[], state='writing')
    rotation['events'].append(event)
    if len(rotation['events']) == 9: rotation['state'] = 'completed'
    data['rotation'] = rotation; _seal(data)
    return event


def _validate_ack(device, data):
    words = dict(data['prior_ack_words']); channel = step = 0; due = acknowledged = None
    previous = data['completed_tick']; delivered = data['state'] in ('delivered', 'acknowledged')
    expected = (0, 1, 0, 0) if data['latch']['pair_id'] == 1 else (0, 0, 0, 0)
    if tuple(words.values()) != expected or type(data['events']) is not list or len(data['events']) > EVENT_LIMIT:
        raise UnsupportedDevice('AF paired ACK initial latch/event budget changed')
    for event in data['events']:
        if (not delivered or type(event) is not dict or set(event) != {'operation', 'cpu', 'address', 'word', 'tick'}
                or channel >= 2 or type(event['tick']) is not int or not previous <= event['tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('AF paired ACK event identity/order changed')
        previous = event['tick']; clear = device.BASE + 0x88c + channel * 0x100; flag = clear + 0x68
        kind, address, word = event['operation'], event['address'], event['word']
        if type(word) is not int or word not in (0, 1): raise UnsupportedDevice('AF paired ACK word domain changed')
        if kind == 'response':
            if event['cpu'] is not None or step not in (2, 3) or due is None or address != flag or word != 0 or previous < due:
                raise UnsupportedDevice('AF paired ACK response lacks its actual request')
            words[flag] = 0; due = None
        else:
            if type(event['cpu']) is not int or event['cpu'] != 0: raise UnsupportedDevice('AF paired ACK CPU owner changed')
            if kind == 'write' and step == 0 and address == clear and word == 0: step = 1; words[clear] = 0
            elif kind == 'write' and step == 1 and address == flag and word == 1: step = 2; words[flag] = 1; due = previous + device.ack_ticks
            elif kind == 'read' and step == 2 and address == clear and word == 0: step = 3
            elif kind == 'read' and step == 3 and address == flag and word == words[flag]:
                if word == 1: step = 2
                else: channel += 1; step = 0; acknowledged = previous if channel == 2 else None
            else: raise UnsupportedDevice('AF paired ACK skips/reorders its actual ISR transaction')
    if (data['ack_channel'] != channel or data['ack_step'] != step or data['ack_due_tick'] != due
            or data['acknowledged_tick'] != acknowledged or data['ack_words'] != words
            or (data['state'] == 'acknowledged') != (channel == 2)):
        raise UnsupportedDevice('AF paired ACK projection differs from the retained driver')


def access(device, operation, core, address, size, value=None):
    record = _owner(device); data = record.get('data')
    if data is None or address not in tuple(device.BASE + offset for offset in ACK_OFFSETS): return False
    validate(device, check_source=False)
    if data['state'] != 'delivered' or core != 0 or type(core) is not int or size != 4 or type(size) is not int:
        raise UnsupportedDevice('AF paired ACK requires delivered data and its CPU0 WORD owner')
    channel, step = data['ack_channel'], data['ack_step']; clear = device.BASE + 0x88c + channel * 0x100; flag = clear + 0x68
    word = device.registers[address] if operation == 'read' else value
    if device.registers[address] != data['ack_words'][address] or type(word) is not int or word not in (0, 1):
        raise UnsupportedDevice('AF live ACK latch changed')
    if operation == 'write' and step == 0 and address == clear and word == 0: next_step = 1
    elif operation == 'write' and step == 1 and address == flag and word == 1: next_step = 2
    elif operation == 'read' and step == 2 and address == clear and word == 0: next_step = 3
    elif operation == 'read' and step == 3 and address == flag and word == data['ack_words'][address]: next_step = 2 if word else 0
    else: raise UnsupportedDevice('AF paired ACK requires the next actual transaction')
    now = device.board.virtual_ticks
    data['events'].append(dict(operation=operation, cpu=core, address=address, word=word, tick=now)); data['ack_step'] = next_step
    if operation == 'write':
        old = device.registers[address]; device.registers[address] = data['ack_words'][address] = word
        device.writes.append(dict(cpu=core, address=hex(address), value=hex(word), old=hex(old)))
        if step == 1: data['ack_due_tick'] = now + device.ack_ticks
    if operation == 'read' and address == flag and word == 0:
        data['ack_channel'] += 1
        if data['ack_channel'] == 2: data.update(state='acknowledged', acknowledged_tick=now)
    _seal(data); return True


def tick(device, board):
    record = _owner(device); data = record.get('data')
    if data is None or data.get('state') == 'acknowledged': return False
    if board is not device.board: raise UnsupportedDevice('AF numeric data belongs to another board')
    now = board.virtual_ticks
    if data['state'] == 'waiting-pair':
        # No timer/effect is owned while waiting. A cheap stock progress/EOI
        # gate avoids repeated historical replay on ordinary execution quanta.
        pair_id = data['latch']['pair_id']
        if int.from_bytes(_read(board, 0x16223fc, 2), 'little') < pair_id: return True
        if any(bank.pending & ((1 << 0x27) | (1 << 0x29)) or bank.active in (0x27, 0x29)
               or bank.injected in (0x27, 0x29) for bank in board.interrupts.banks): return True
        pair = _pair(board, pair_id)
        if pair is None: return True
        validate(device); _prepare(device, data, pair)
    elif data['state'] == 'pending' and now < data['due_tick']: return True
    elif data['state'] == 'delivered' and (data['ack_due_tick'] is None or now < data['ack_due_tick']): return True
    validate(device)
    if data['state'] == 'pending' and now >= data['due_tick']:
        validate(device, check_route=True)
        for span in data['result']['plan']: board.write_memory(span['address'], bytes.fromhex(span['payload_hex']), cpu_id=0)
        data.update(state='delivered', completed_tick=now); _seal(data); board.queue_irq(0, 46)
        board.record('First contrast AF scene delivered before IRQ46', profile=PROFILE,
                     sampled_focus=data['pair']['sampled_focus'], limits=data['result']['assumptions'])
    elif data['state'] == 'delivered' and data['ack_due_tick'] is not None and now >= data['ack_due_tick']:
        address = device.BASE + 0x8f4 + data['ack_channel'] * 0x100
        if device.registers[address] != 1 or data['ack_words'][address] != 1:
            raise UnsupportedDevice('AF ACK response lacks its actual flag store')
        data['events'].append(dict(operation='response', cpu=None, address=address, word=0, tick=now))
        device.registers[address] = data['ack_words'][address] = 0; data['ack_due_tick'] = None; _seal(data)
    return data['state'] != 'acknowledged'


def due(device):
    data = _owner(device).get('data')
    if data is None: return None
    return data['due_tick'] if data['state'] == 'pending' else data['ack_due_tick'] if data['state'] == 'delivered' else None


def anchor(device, pair_id=1):
    """Pure closed-data identity for future source guards; no Source recursion."""
    record = _owner(device); history = record.get('data_history', []); current = record.get('data')
    if type(pair_id) is not int or not 1 <= pair_id <= MAX_PAIRS:
        raise UnsupportedDevice('AF data anchor requires a bounded contiguous pair identity')
    if type(history) is not list or len(history) >= MAX_PAIRS:
        raise UnsupportedDevice('AF data anchor lost its flat history')
    data = history[pair_id-1] if pair_id <= len(history) else current
    if (data is None or data.get('latch', {}).get('pair_id') != pair_id or data.get('state') != 'acknowledged'
            or data.get('state_sha256') != hp.digest({key: value for key, value in data.items() if key != 'state_sha256'})
            or data.get('latch_sha256') != hp.digest(data['latch'])):
        raise UnsupportedDevice('AF next pair requires the selected sealed data closure')
    _validate_ack(device, data)
    return deepcopy(dict(profile=data['profile'], pair_id=pair_id, state=data['state'], input_sha256=data['latch']['input_sha256'],
        producer=data['latch']['producer'], pair=data['pair'], acknowledged_tick=data['acknowledged_tick'],
        ack_channel=data['ack_channel'], ack_step=data['ack_step'], ack_due_tick=data['ack_due_tick'],
        ack_words=data['ack_words'], state_sha256=data['state_sha256']))


def summary(device):
    record = device.rearm.get('afs_focus') if type(device.rearm) is dict else None
    if record is None or type(record) is dict and record.get('data') is None:
        return None
    data = validate(device)
    return dict(profile=data['profile'], state=data['state'], pair_id=data['latch']['pair_id'], armed_tick=data['armed_tick'],
        sampled_focus=data['pair']['sampled_focus'] if data['pair'] is not None else None,
        target_position=data['latch']['target_position'], due_tick=data['due_tick'],
        completed_tick=data['completed_tick'], acknowledged_tick=data['acknowledged_tick'],
        ack_channel=data['ack_channel'], ack_step=data['ack_step'], ack_due_tick=data['ack_due_tick'],
        rotation_step=len(data['rotation']['events']) if data['rotation'] is not None else 0,
        limits='Declared normalized ROI squared-gradient data; explicit contiguous sampled pairs; no AF outcome')
