"""Unarmed still numeric scene windows for the actual mask46 initializer.

The monochrome histogram/gradient/sum law is explicitly declared. It is
neither recovered sensor calibration nor a camera result. This preparation
only returns bounded consumer windows; it writes no bytes, timer or IRQ.
The separately named finite transport waits for the genuine readout clock
and client3 contract, then supplies bytes and one physical IRQ46. Stock ARM
owns paired ACK, finite retirement, packets and capture results.
"""
from copy import deepcopy
import hashlib

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from .virtual_source import ram_span
from .afs_scene_fixture import consumer_payloads
from . import producer_still_capture as still


PROFILE = 'declared-still-yycbcr-scene-histogram-gradient-sum-unmeasured'
TRANSPORT_PROFILE = 'declared-still-scene-finite-numeric-dma-irq46-unmeasured'
READOUT_PROFILE = 'declared-afs-generation9-still-readout-start-source1-clock-unmeasured'
ACK_OFFSETS = (0x88c, 0x8f4, 0x98c, 0x9f4)
FINITE_STOP = ((4, 0x33170000), (8, 0x10000), (0x884, 0), (0x8f8, 0))
OFFSETS = (0, 0x800, 0x850, 0x1850, 0x1990, 0x1d90, 0x2190, 0x21b0, 0x23b0)
BANK_SIZE = 0x29b0
ASSUMPTIONS = ('Declared frozen YYCbCr monochrome scene; four equal luma histogram planes, '
    'horizontal absolute gradients and luma sums; other known consumer windows zero. '
    'Sensor crop, CFA, exposure and silicon numerical calibration are unmeasured. '
    'Stock numerical transforms and capture decisions must run unchanged.')


def _scene(board, scene):
    fields = ('address', 'bytes', 'width', 'height', 'stride_bytes', 'payload_sha256')
    if type(scene) is not dict or any(key not in scene for key in fields):
        raise UnsupportedDevice('Still numeric plan requires its explicit frozen YYCbCr scene')
    try:
        address = int(scene['address'], 16) if type(scene['address']) is str else scene['address']
    except ValueError as error:
        raise UnsupportedDevice('Still numeric scene address is invalid') from error
    size = scene['bytes']
    if type(address) is not int or type(size) is not int or not 1 <= size <= 4 * 1024 * 1024:
        raise UnsupportedDevice('Still numeric scene exceeds its bounded DRAM span')
    span = ram_span(address, size)
    image = bytes(board.read_memory(address, size, cpu_id=0))
    if hashlib.sha256(image).hexdigest() != scene['payload_sha256']:
        raise UnsupportedDevice('Still numeric scene differs from its retained byte hash')
    payload = consumer_payloads(image, width=scene['width'], height=scene['height'],
                                stride_bytes=scene['stride_bytes'])
    return {key: deepcopy(scene[key]) for key in fields}, span, payload


def prepare(device, *, scene, profile=None):
    """Return an initializer-bound plan without arming any physical output."""
    if profile != PROFILE:
        raise UnsupportedDevice('Still numeric plan requires the explicit declared scene profile')
    activation = still.source_anchor(device)
    registers = device.registers
    destinations = {address: registers[address] for address in device.DESTINATIONS}
    bank = destinations[device.BASE + 0x500]
    if (type(bank) is not int or bank & 15 or tuple(destinations.values()) !=
            tuple(bank + offset for offset in OFFSETS)
            or registers.get(device.BASE + 0x524) != 0x200
            or int.from_bytes(device.board.read_memory(0x15f6bf8, 4), 'little') != bank):
        raise UnsupportedDevice('Still numeric plan requires the unchanged completed bank and nine actual destinations')
    bank_span = ram_span(bank, BANK_SIZE)
    source, source_span, payload = _scene(device.board, scene)
    if bank_span[0] < source_span[1] and source_span[0] < bank_span[1]:
        raise UnsupportedDevice('Still numeric output bank overlaps its frozen scene')
    histogram = next(raw for register, offset, raw in payload['spans']
                     if (register, offset) == (device.BASE + 0x508, 0))
    plan, spans = [], []
    for register, offset, raw in payload['spans']:
        if (register, offset) == (device.BASE + 0x520, 0x200):
            raw = histogram
        address = destinations[register] + offset
        low, high = ram_span(address, len(raw))
        if (address & 3 or not bank_span[0] <= low < high <= bank_span[1]
                or any(low < end and start < high for start, end in spans)):
            raise UnsupportedDevice('Still numeric consumer windows overlap or exceed the retained bank')
        spans.append((low, high))
        plan.append(dict(register=register, offset=offset, address=address,
                         bytes=len(raw), payload_hex=raw.hex()))
    result = dict(profile=profile, state='unarmed', activation=activation, bank=bank,
        destinations=destinations, desired={address: registers[address] for address in device.KNOWN_DESIRED_CONFIG},
        descriptor_sha256=hashlib.sha256(device.board.read_memory(0x15f6c8c, 0xd8)).hexdigest(),
        scene=source, plan=plan, pixels=payload['pixels'], luma_sum=payload['luma_sum'],
        bytes=sum(span['bytes'] for span in plan), assumptions=ASSUMPTIONS)
    result['input_sha256'] = digest(result)
    return result


def admission(device, *, scene, profile=None):
    """Pure readiness; a valid plan grants no physical completion permission."""
    try:
        result = prepare(device, scene=scene, profile=profile)
    except UnsupportedDevice as error:
        return dict(ready=False, state='unarmed', reason=str(error))
    return dict(ready=True, state='unarmed', profile=PROFILE, bank=result['bank'],
                bytes=result['bytes'], input_sha256=result['input_sha256'],
                reason='Scene windows prepared; finite readout epoch and live transport are not admitted')


def _owner(device):
    return device.rearm.get(still.KEY, {}).get('data') if type(device.rearm) is dict else None


def _u(device, address, size=4):
    return int.from_bytes(device.board.read_memory(address, size, cpu_id=0), 'little')


def _readout(device):
    sources = [item for item in device.board.devices if type(item).__name__ == 'AfsFrameSource']
    if len(sources) != 1 or not callable(getattr(sources[0], 'still_readout_clock_summary', None)):
        raise UnsupportedDevice('Still numeric transport requires its real readout clock owner')
    marker = sources[0].still_readout_clock_summary()
    if (type(marker) is not dict or tuple(marker.get(key) for key in
            ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'state')) !=
            (READOUT_PROFILE, 9, 0, 1, '0x29', 'acknowledged')
            or type(marker.get('eoi_tick')) is not int):
        raise UnsupportedDevice('Still numeric transport awaits physical source1 readout ACK and EOI')
    fields = ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'input_sha256',
              'asserted_tick', 'acknowledged_tick', 'eoi_tick')
    return {key: deepcopy(marker[key]) for key in fields}, marker['contract']['producer']


def _seal(data):
    data['state_sha256'] = digest({key: value for key, value in data.items() if key != 'state_sha256'})


def _drained(device):
    return all(not bank.pending & (1 << 46) and bank.active != 46 and bank.injected != 46
               and not any(entry.get('line') == 46 for entry in bank.active_stack)
               for bank in device.board.interrupts.banks)


def _live(device, data=None):
    fields = ((0x15e32d0, 4, 12), (0x15d502c, 4, 7), (0x15d5090, 1, 1),
              (0x15d5091, 1, 1), (0x15d5093, 1, 0), (0x15d5098, 4, 0),
              (0x15f68a4, 4, 3), (0x15f68a8, 4, 0x12f604c), (0x15f6bf4, 4, 1))
    if any(_u(device, address, size) != value for address, size, value in fields):
        raise UnsupportedDevice('Still numeric transport requires the genuine one-result finite readout request')
    if not _drained(device):
        raise UnsupportedDevice('Still numeric transport requires a drained physical IRQ46')
    route(device, pending=False)
    if data is not None:
        plan = data['plan']
        scene = plan['scene']
        source = int(scene['address'], 16) if type(scene['address']) is str else scene['address']
        if (_u(device, 0x15f6bf8) != plan['bank']
                or any(device.registers[address] != value for address, value in plan['destinations'].items())
                or any(device.registers[address] != value for address, value in plan['desired'].items())
                or hashlib.sha256(device.board.read_memory(0x15f6c8c, 0xd8)).hexdigest() != plan['descriptor_sha256']):
            raise UnsupportedDevice('Still numeric destinations, desired configuration or descriptor changed before DMA')
        if hashlib.sha256(device.board.read_memory(source, scene['bytes'])).hexdigest() != scene['payload_sha256']:
            raise UnsupportedDevice('Still numeric frozen scene bytes changed before DMA')


def arm(device, *, scene, profile=None, delay_ticks=1024):
    """Declare one future finite transport; admission has no guest effects."""
    if (profile != TRANSPORT_PROFILE or _owner(device) is not None or device.board.failure is not None
            or type(delay_ticks) is not int or not 1 <= delay_ticks <= 400000):
        raise UnsupportedDevice('Still numeric finite transport requires its explicit one-shot profile and delay')
    still.validate(device)
    epoch, activation = _readout(device)
    _live(device)
    plan = prepare(device, scene=scene, profile=PROFILE)
    if activation != plan['activation']:
        raise UnsupportedDevice('Still numeric readout clock belongs to another initializer')
    now = device.board.virtual_ticks
    if not 0 <= plan['activation']['enabled_tick'] < epoch['asserted_tick'] <= epoch['eoi_tick'] <= now < (1 << 64) - delay_ticks:
        raise UnsupportedDevice('Still numeric transport must follow its actual initializer and readout clock')
    prior = {device.BASE + offset: device.registers[device.BASE + offset] for offset in ACK_OFFSETS}
    if tuple(prior.values()) != (0, 1, 0, 0):
        raise UnsupportedDevice('Still numeric initial paired ACK latches differ from the whole initializer')
    retirement = dict(state='waiting', step=0, accesses=[],
                      initial_words={device.BASE + offset: device.registers[device.BASE + offset]
                                     for offset, _ in FINITE_STOP})
    retirement['words'] = dict(retirement['initial_words'])
    data = dict(profile=profile, sample=1, state='pending', armed_tick=now, due_tick=now + delay_ticks,
        delay_ticks=delay_ticks, completed_tick=None, acknowledged_tick=None, plan=plan, plan_sha256=digest(plan),
        readout=epoch, readout_sha256=digest(epoch), prior_ack_words=prior, ack_words=dict(prior),
        ack_channel=0, ack_step=0, ack_due_tick=None, events=[], retirement=retirement)
    _seal(data)
    device.rearm[still.KEY]['data'] = data
    device.board.record('Still numeric finite scene transport explicitly armed', profile=profile,
        due_tick=data['due_tick'], bytes=plan['bytes'], limits=ASSUMPTIONS)
    return summary(device)


def _validate_retirement(device, data):
    close = data['retirement']
    if (type(close) is not dict or set(close) != {'state', 'step', 'accesses', 'initial_words', 'words'}
            or type(close['step']) is not int or not 0 <= close['step'] <= len(FINITE_STOP)
            or close['state'] != ('complete' if close['step'] == len(FINITE_STOP) else 'waiting')
            or type(close['accesses']) is not list or len(close['accesses']) != close['step']
            or set(close['initial_words']) != {device.BASE + offset for offset, _ in FINITE_STOP}):
        raise UnsupportedDevice('Still numeric finite retirement schema or bounds changed')
    words = dict(close['initial_words'])
    previous = data['acknowledged_tick']
    for event, (offset, value) in zip(close['accesses'], FINITE_STOP):
        if (type(event) is not dict or set(event) != {'cpu', 'address', 'word', 'old_word', 'tick'}
                or event['cpu'] != 0 or type(event['cpu']) is not int
                or (event['address'], event['word'], event['old_word']) !=
                   (device.BASE + offset, value, words[device.BASE + offset])
                or type(event['tick']) is not int or previous is None
                or not previous <= event['tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Still numeric finite retirement differs from the whole four-store ISR')
        words[event['address']] = value
        previous = event['tick']
    if words != close['words']:
        raise UnsupportedDevice('Still numeric finite retirement WORD projection changed')


def validate(device, *, check_source=True):
    data = _owner(device)
    if data is None: return None
    fields = {'profile', 'sample', 'state', 'armed_tick', 'due_tick', 'delay_ticks', 'completed_tick',
        'acknowledged_tick', 'plan', 'plan_sha256', 'readout', 'readout_sha256', 'prior_ack_words',
        'ack_words', 'ack_channel', 'ack_step', 'ack_due_tick', 'events', 'retirement', 'state_sha256'}
    if (type(data) is not dict or set(data) != fields or data['profile'] != TRANSPORT_PROFILE
            or type(data['sample']) is not int or data['sample'] != 1
            or data['state'] not in ('pending', 'delivered', 'acknowledged')
            or data['state_sha256'] != digest({key: value for key, value in data.items() if key != 'state_sha256'})
            or data['plan_sha256'] != digest(data['plan']) or data['readout_sha256'] != digest(data['readout'])
            or data['plan']['activation'] != still.source_anchor(device)
            or type(data['armed_tick']) is not int or type(data['delay_ticks']) is not int
            or not 1 <= data['delay_ticks'] <= 400000
            or not 0 <= data['armed_tick'] < data['due_tick'] == data['armed_tick'] + data['delay_ticks'] < 1 << 64
            or not data['readout']['eoi_tick'] <= data['armed_tick'] <= device.board.virtual_ticks
            or type(data['events']) is not list or len(data['events']) > 4096):
        raise UnsupportedDevice('Still numeric transport seal, source, initializer or timeline changed')
    if check_source and _readout(device) != (data['readout'], data['plan']['activation']):
        raise UnsupportedDevice('Still numeric readout history changed after admission')
    if data['state'] == 'pending':
        if data['completed_tick'] is not None or data['acknowledged_tick'] is not None:
            raise UnsupportedDevice('Still numeric transport claims early physical completion')
    elif type(data['completed_tick']) is not int or not data['due_tick'] <= data['completed_tick'] <= device.board.virtual_ticks:
        raise UnsupportedDevice('Still numeric DMA precedes its declared delay')
    # Both families use the same audited92C8CC two-channel ACK protocol.
    from .producer_capture_metstats import _validate_ack
    _validate_ack(device, data)
    _validate_retirement(device, data)
    return data


def words(device):
    data = _owner(device)
    return {**data['ack_words'], **data['retirement']['words']} if data is not None else {}


def route(device, *, pending=None):
    data = _owner(device)
    if data is not None and data['retirement']['state'] == 'complete':
        if not any(bank.enabled & (1 << 46) for bank in device.board.interrupts.banks):
            banks = device.board.interrupts.banks
            if (any(bank.pending & (1 << 46) or bank.injected == 46 for bank in banks)
                    or any(bank.active == 46 or any(entry.get('line') == 46 for entry in bank.active_stack)
                           for bank in banks[1:])):
                raise UnsupportedDevice('Retired still numeric IRQ46 cannot be newly pending or staged')
            return None
    from .afs_statistics_frame import _route
    _route(device, pending=(data is not None and data['state'] in ('delivered', 'acknowledged')) if pending is None else pending)
    return dict(cpu=0, vector=0x92c8cc, priority=13, source_configuration=1)


def access(device, operation, cpu, address, size, value=None):
    data = _owner(device)
    if data is None: return False
    ack_addresses = {device.BASE + offset for offset in ACK_OFFSETS}
    retirement = data['retirement']
    if address not in ack_addresses and address not in retirement['words']: return False
    if type(cpu) is not int or cpu != 0 or type(size) is not int or size != 4:
        raise UnsupportedDevice('Still numeric ACK and retirement require real CPU0 WORD transactions')
    validate(device, check_source=False)
    word = device.registers[address] if operation == 'read' else value
    if address in ack_addresses:
        if data['state'] == 'acknowledged' and operation == 'read' and word == data['ack_words'][address]:
            return True
        if data['state'] != 'delivered' or device.registers[address] != data['ack_words'][address]:
            raise UnsupportedDevice('Still numeric ACK requires delivered bytes and unchanged hardware')
        channel, step = data['ack_channel'], data['ack_step']
        clear = device.BASE + 0x88c + channel * 0x100
        flag = clear + 0x68
        if operation == 'write' and step == 0 and address == clear and word == 0: next_step = 1
        elif operation == 'write' and step == 1 and address == flag and word == 1: next_step = 2
        elif operation == 'read' and step == 2 and address == clear and word == 0: next_step = 3
        elif operation == 'read' and step == 3 and address == flag and word == data['ack_words'][address]: next_step = 2 if word else 0
        else: raise UnsupportedDevice('Still numeric ACK must perform its next ordered paired transaction')
        data['events'].append(dict(operation=operation, cpu=cpu, address=address, word=word, tick=device.board.virtual_ticks))
        data['ack_step'] = next_step
        if operation == 'write':
            device.registers[address] = word; data['ack_words'][address] = word
            if word == 1: data['ack_due_tick'] = device.board.virtual_ticks + device.ack_ticks
        if operation == 'read' and step == 3 and not word:
            data['ack_channel'] += 1
            if data['ack_channel'] == 2: data.update(state='acknowledged', acknowledged_tick=device.board.virtual_ticks)
    elif data['state'] == 'acknowledged':
        if operation == 'read' and word == retirement['words'][address]: return True
        step = retirement['step']
        if (operation != 'write' or step >= len(FINITE_STOP)
                or (address, word) != (device.BASE + FINITE_STOP[step][0], FINITE_STOP[step][1])
                or device.registers[address] != retirement['words'][address]):
            raise UnsupportedDevice('Still numeric must perform its next whole finite retirement store')
        retirement['accesses'].append(dict(cpu=cpu, address=address, word=word,
            old_word=device.registers[address], tick=device.board.virtual_ticks))
        retirement['words'][address] = word; device.registers[address] = word
        retirement['step'] += 1
        if retirement['step'] == len(FINITE_STOP): retirement['state'] = 'complete'
    else: return False
    if operation == 'write':
        device.writes.append(dict(cpu=cpu, address=hex(address), value=hex(word)))
    _seal(data)
    return True


def tick(device, board):
    data = _owner(device)
    if data is None or data['state'] == 'acknowledged': return False
    due_tick = data['ack_due_tick'] if data['ack_due_tick'] is not None else data['due_tick'] if data['state'] == 'pending' else None
    if due_tick is None or board.virtual_ticks < due_tick: return False
    still.validate(device)
    if data['state'] == 'pending':
        _live(device, data)
        for span in data['plan']['plan']:
            board.write_memory(span['address'], bytes.fromhex(span['payload_hex']))
        data.update(state='delivered', completed_tick=board.virtual_ticks)
        _seal(data)
        board.queue_irq(0, 46)
        board.record('Declared still numeric scene bytes and physical IRQ46 delivered',
                     bytes=data['plan']['bytes'], limits=ASSUMPTIONS)
    else:
        flag = device.BASE + 0x8f4 + data['ack_channel'] * 0x100
        data['events'].append(dict(operation='response', cpu=None, address=flag, word=0, tick=board.virtual_ticks))
        data['ack_words'][flag] = 0; device.registers[flag] = 0; data['ack_due_tick'] = None
        _seal(data)
    return True


def due(device):
    data = _owner(device)
    if data is None or data['state'] == 'acknowledged': return None
    return data['ack_due_tick'] if data['ack_due_tick'] is not None else data['due_tick'] if data['state'] == 'pending' else None


def anchor(device):
    """Pure sealed closure; no Source/public-validation recursion."""
    data = validate(device, check_source=False)
    if data is None or data['state'] != 'acknowledged' or data['retirement']['state'] != 'complete':
        raise UnsupportedDevice('Still numeric closure awaits real paired ACK and finite retirement')
    return {key: deepcopy(data[key]) for key in ('profile', 'state', 'completed_tick', 'acknowledged_tick',
        'readout', 'plan_sha256', 'state_sha256')}


def summary(device):
    data = validate(device)
    if data is None: return None
    return dict(profile=data['profile'], state=data['state'], bank=data['plan']['bank'],
        bytes=data['plan']['bytes'], due_tick=due(device), completed_tick=data['completed_tick'],
        acknowledged_tick=data['acknowledged_tick'], retirement=data['retirement']['state'],
        retirement_stores=data['retirement']['step'])
