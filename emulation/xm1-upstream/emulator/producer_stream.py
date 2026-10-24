"""Continuous statistics producer for the opt-in sensor stream.

The strict single-lifetime helpers (``afs_statistics_frame``,
``producer_measurement`` ...) each own one declared frame. This helper
generalizes their register/ACK contracts to a repeating frame, only while the
host has selected the stream (``emulator/testbed/STREAM.md``):

* the nine destination registers are latched when the source0 edge asserts
  (the hardware fills the bank programmed before the edge; the stock
  source callback then programs the next frame's bank);
* a scene source's bytes are written into those latched windows, then IRQ
  0x2E is asserted, only if the firmware's own producer enable/route is
  live and the previous frame's paired ACK has finished;
* paired clear/flag response latency is unmeasured.

Nothing is written into firmware state other than DMA'd bank bytes before
the IRQ. Rotation stores follow the exact stock nine-word order but are not
recorded in the finite ``rearm['rotations']`` ledger.
"""
from dataclasses import dataclass
from enum import Enum
import hashlib

from .hardware import UnsupportedDevice
from .interrupts import IrqRoute
from .virtual_source import FrameIndex, VirtualTick, ram_span, xm1_source_tick

PROFILE = 'declared-testbed-continuous-statistics-stream-unmeasured'


class ProducerStreamProfile(str, Enum):
    XM1 = 'xm1-150-declared-continuous-statistics-scene-unmeasured'


@dataclass(frozen=True)
class StatisticsFrame:
    index: FrameIndex
    tick: VirtualTick
    latched_tick: VirtualTick

    def __post_init__(self):
        if (type(self.index) is not int or self.index < 1
                or type(self.tick) is not int or self.tick < 0
                or type(self.latched_tick) is not int or self.latched_tick < self.tick):
            raise ValueError('Invalid statistics frame identity')


XM1_IRQ_HANDLER = 0x67bfdc
XM1_SAMPLING_OFFSETS = (
    0x100, 0x104, 0x108, 0x15c,
    *range(0x10c, 0x15c, 4),
    0x200, 0x204, 0x258,
    *range(0x208, 0x258, 4),
    0x300, 0x304, 0x30c,
    0x400, 0x404, 0x408, 0x40c, 0x410, 0x414,
    0x604, 0x608,
)
XM1_WINDOW_BYTES = (0x800, 0x50, 0x1000, 0x140, 0x400, 0x400, 0x20, 0x200, 0x200)

ACK_OFFSETS = (0x88c, 0x8f4, 0x98c, 0x9f4)
IRQ = 0x2e
IRQ_HANDLER = 0x92c8cc
LOG_LIMIT = 64


def enable(device, *, data_delay_ticks=20000, profile: ProducerStreamProfile | None = None):
    if device.stream is not None:
        raise UnsupportedDevice('Continuous statistics stream is already enabled')
    if type(data_delay_ticks) is not int or data_delay_ticks < 1:
        raise UnsupportedDevice('Statistics data delay must be a positive integer')
    if profile is not None and type(profile) is not ProducerStreamProfile:
        raise UnsupportedDevice('Unknown continuous producer profile')
    if profile is ProducerStreamProfile.XM1:
        for offset in ACK_OFFSETS:
            device.registers.setdefault(device.BASE + offset, 0)
    device.stream = dict(
        profile=PROFILE if profile is None else profile, data_delay_ticks=data_delay_ticks, rotation_step=0, rotations=0,
        tuple=None, frame=None, state='idle', due_tick=None, completed_tick=None,
        ack_channel=0, ack_step=0, ack_due_tick=None, ack_words={}, ack_log=[],
        frames_latched=0, config_stores=0, histogram_destination=None, hash_dirty=False, frames_delivered=0, frames_acknowledged=0,
        skipped_disabled=0, skipped_busy=0, skipped_route=0, bytes_delivered=0,
        last_scene=None, source_tick=None, source_index=FrameIndex(0),
        sampling_words=None, log=[])
    device.board.record('Continuous statistics stream enabled', profile=device.stream['profile'],
        data_delay_ticks=data_delay_ticks,
        limits=('Declared analytic X-M1 statistics into source-latched windows before its IRQ2E; real guest paired ACK and callback paths; numerical laws, field semantics and timing are unmeasured'
                if profile is ProducerStreamProfile.XM1 else
                'Declared scene bytes into firmware-latched bank windows before IRQ 0x2E; stock paired ACK; unmeasured timing'))


def disable(device):
    device.stream = None


def live(device):
    """The firmware has armed and enabled the statistics producer.

    Derived from register words alone (the stock arm/enable sequences set
    them), so it follows the firmware through stop/re-arm/mode changes.
    """
    r = device.registers
    return (r[device.BASE + 4] == device.PRODUCER_SELECT_MASK and r[device.BASE + 0x884] == 1
            and r[device.BASE + 0x8f8] == 0x10000000
            and all(r[a] for a in device.DESTINATIONS))


def _route(device):
    interrupts = device.board.interrupts
    if interrupts.route(IRQ, IrqRoute.Every) != [0]:
        return None
    bank = interrupts.banks[0]
    handler = XM1_IRQ_HANDLER if device.stream['profile'] is ProducerStreamProfile.XM1 else IRQ_HANDLER
    if (bank.config.get(0x400 + IRQ * 4) != handler or not bank.source_config & (1 << IRQ)
            or bank.pending & (1 << IRQ) or bank.active == IRQ or bank.injected == IRQ):
        return None
    return 0


def latch_frame(device, frame, tick):
    """Capture the bank tuple the hardware will fill for this source edge."""
    s = device.stream
    if s is None:
        return False
    if s['state'] != 'idle':
        s['skipped_busy'] += 1
        return False
    if not live(device):
        s['skipped_disabled'] += 1
        return False
    if s['profile'] is ProducerStreamProfile.XM1 and s.get('control_ack_due'):
        s['skipped_busy'] += 1
        return False
    if s['profile'] is ProducerStreamProfile.XM1 and type(frame) is not StatisticsFrame:
        raise UnsupportedDevice('X-M1 statistics requires a typed physical frame')
    s['tuple'] = {a: device.registers[a] for a in device.DESTINATIONS}
    r = device.registers
    s['histogram_destination'] = (r[device.BASE + 0x700] if r[device.BASE + 0x984] == 1
                                  and r[device.BASE + 0x9f8] == 0x10000000 else None)
    if s['profile'] is ProducerStreamProfile.XM1 and s['histogram_destination'] is not None:
        s['tuple'][device.BASE + 0x700] = s['histogram_destination']
    if s['profile'] is ProducerStreamProfile.XM1:
        s['frame'] = frame
        s['sampling_words'] = tuple(r[device.BASE + offset] for offset in XM1_SAMPLING_OFFSETS)
    else:
        s['frame'] = dict(frame, latched_tick=tick)
    s['state'] = 'latched'
    s['due_tick'] = tick + s['data_delay_ticks']
    s['frames_latched'] += 1
    return True


def _plan(device, spans, tuple_words):
    plan, ranges = [], []
    for register, offset, raw in spans:
        base = tuple_words.get(register)
        if base is None:
            raise UnsupportedDevice('Scene statistics span names an unknown destination register')
        address = base + offset
        lo, hi = ram_span(address, len(raw))
        if address & 3 or len(raw) & 3 or any(lo < h and l < hi for l, h in ranges):
            raise UnsupportedDevice('Scene statistics span is unaligned or overlaps another span')
        ranges.append((lo, hi))
        plan.append((address, raw))
    return plan


def _latch_xm1_clock(device, board):
    s = device.stream
    tick = xm1_source_tick(board)
    if tick is None or tick == s['source_tick']:
        return
    s['source_tick'] = tick
    s['source_index'] = FrameIndex(s['source_index'] + 1)
    frame = StatisticsFrame(s['source_index'], tick, VirtualTick(board.virtual_ticks))
    latch_frame(device, frame, tick)


def _xm1_payloads(device):
    from x70_native._native import virtual_scene_xm1_statistics
    s = device.stream
    buffers = virtual_scene_xm1_statistics(s['frame'].index, s['sampling_words'],
                                          s['histogram_destination'] is not None)
    if len(buffers) != len(XM1_WINDOW_BYTES) + 1:
        raise UnsupportedDevice('X-M1 statistics window count differs')
    spans = []
    for register, expected, data in zip(device.DESTINATIONS, XM1_WINDOW_BYTES, buffers):
        if type(data) is not bytes or len(data) != expected:
            raise UnsupportedDevice('X-M1 statistics window byte extent differs')
        spans.append((register, 0, data))
    if s['histogram_destination'] is not None:
        data = buffers[-1]
        if type(data) is not bytes or len(data) != 0x400:
            raise UnsupportedDevice('X-M1 histogram byte extent differs')
        spans.append((device.BASE + 0x700, 0, data))
    return spans


def tick(device, board):
    s = device.stream
    if s is None:
        return False
    if s.get('stop_due') is not None and board.virtual_ticks >= s['stop_due']:
        device.registers[device.GLOBAL] = 0x10000000
        s['stop_due'] = None
    if s['profile'] is ProducerStreamProfile.XM1:
        _latch_xm1_clock(device, board)
        for address, deadline in tuple(s.get('control_ack_due', {}).items()):
            if board.virtual_ticks >= deadline:
                device.registers[address] = 0
                del s['control_ack_due'][address]
    if s['state'] == 'latched' and board.virtual_ticks >= s['due_tick']:
        route = _route(device) if live(device) else None
        if route is None:
            # The firmware withdrew the producer/IRQ: the hardware would not
            # have transferred; drop the frame without effects.
            s['skipped_route'] += 1
            s['state'] = 'idle'
            return False
        host = getattr(board, 'sensor_stream', None)
        if s['profile'] is ProducerStreamProfile.XM1:
            spans = _xm1_payloads(device)
        else:
            if host is None:
                raise UnsupportedDevice('Continuous statistics stream has no scene host')
            spans = host.statistics(s['frame'])
        plan = _plan(device, spans, s['tuple'])
        total = 0
        for address, raw in plan:
            board.write_memory(address, raw, cpu_id=0)
            total += len(raw)
        if s['histogram_destination'] is not None:
            if s['profile'] is not ProducerStreamProfile.XM1:
                data = host.histogram(s['frame'], spans)
                ram_span(s['histogram_destination'], len(data))
                board.write_memory(s['histogram_destination'], data, cpu_id=0)
                total += len(data)
            s['histograms_delivered'] = s.get('histograms_delivered', 0) + 1
        digest = hashlib.sha256(b''.join(raw for _, raw in plan)).hexdigest()
        index = s['frame'].index if s['profile'] is ProducerStreamProfile.XM1 else s['frame']['index']
        s.update(state='delivered', completed_tick=board.virtual_ticks, ack_channel=0, ack_step=0,
                 ack_due_tick=None,
                 last_scene=dict(frame=index, bytes=total, sha256=digest))
        s['ack_words'] = {a: device.registers[a] for a in (device.BASE + o for o in ACK_OFFSETS)}
        s['frames_delivered'] += 1
        s['bytes_delivered'] += total
        board.queue_irq(route, IRQ)
    elif s['state'] == 'delivered' and s['ack_due_tick'] is not None and board.virtual_ticks >= s['ack_due_tick']:
        address = device.BASE + 0x8f4 + s['ack_channel'] * 0x100
        if device.registers[address] != 1:
            raise UnsupportedDevice('Stream ACK response lacks its actual request')
        device.registers[address] = 0
        s['ack_due_tick'] = None
    return False


def due(device):
    s = device.stream
    if s is None:
        return None
    times = [s['stop_due']] if s.get('stop_due') is not None else []
    if s['state'] == 'latched':
        times.append(s['due_tick'])
    if s['state'] == 'delivered' and s['ack_due_tick'] is not None:
        times.append(s['ack_due_tick'])
    times.extend(s.get('control_ack_due', {}).values())
    return min(times) if times else None


def refresh_hashes(device):
    """Re-seal the frozen ledger's configuration hashes after live stores.

    Strict helpers that still run while streaming (histogram, shutdown
    prefixes) compare these hashes with the registers; the stream owns the
    live register words, so it reseals them lazily rather than per store.
    """
    s = device.stream
    if s is not None and s.get('hash_dirty') and type(device.rearm) is dict:
        device.rearm['configuration_sha256'] = device._config_hash()
        device.rearm['retained_configuration_sha256'] = device._retained_config_hash()
        s['hash_dirty'] = False


def _rotation_access(device, operation, core, address, value):
    """Count stock nine-store destination rotations while live; any other
    destination store (descriptor rebuild, out-of-order) is a plain store."""
    s = device.stream
    order = device.REARM_ROTATION_ORDER
    if operation == 'read' or address not in order:
        return False
    if not live(device) or order[s['rotation_step']] != address:
        s['rotation_step'] = 0
        return False
    if not device._config_valid(address, value):
        raise UnsupportedDevice('Producer destination store has unknown bits/domain')
    _store(device, core, address, value)
    s['rotation_step'] += 1
    if s['rotation_step'] == len(order):
        s['rotation_step'] = 0
        s['rotations'] += 1
    return True


def is_extra_word(device, address):
    """Uncatalogued retained words of the global window (GLOBAL+0x10..0x33fc)."""
    return (device.GLOBAL + 4 <= address < device.GLOBAL + 0x3400 and address & 3 == 0
            and address not in device.ALL_CONFIG_MASKS and address != device.GLOBAL)


def _store(device, core, address, value):
    old = device.registers[address]
    device.registers[address] = value
    device.writes.append(dict(cpu=core, address=hex(address), value=hex(value), old=hex(old)))
    del device.writes[:-256]
    device.stream['hash_dirty'] = True


def _register_file(device, operation, core, address, value):
    """Permissive producer register file for the repeating firmware traffic.

    The finite rearm ledgers audit every access against an O(history) chain
    (tens of milliseconds, 8192-store budget) and admit exactly one lifetime.
    While streaming, each word is instead checked against its stock
    mask/domain and the stock stop/arm/enable values; ordering is not
    audited and the ledgers stay frozen at enable. Channel 1 (0x98x/0x9fx,
    histogram) stays with the histogram mixin. Status words: the idle poll 0x890 reads 0, and the global stop (write 0) sets bit 28
    after the stop delay, as the strict stop profile does.
    """
    r, B, s = device.registers, device.BASE, device.stream
    xm1 = s['profile'] is ProducerStreamProfile.XM1
    if operation == 'read':
        if not xm1 and is_extra_word(device, address):
            r.setdefault(address, 0)
            return True
        if address == B + 0x990:
            return True
        if address in device.ALL_CONFIG_MASKS and not (address == B + 0x524 and r[address] == 0):
            if not device._config_valid(address, r[address]):
                raise UnsupportedDevice('Producer configuration word has an unmodeled value')
        return address in r
    allowed = {B + 4: (0, device.PRODUCER_SELECT_MASK, device.PRODUCER_DISABLED_MASK),
               B + 8: (0, 1, 0x10000, 0x10001),
               B: (0,), B + 0x880: (0,), B + 0x980: (0,), B + 0x884: (0, 1),
               B + 0x88c: (0, 1), B + 0x8f4: (0, 1), B + 0x8a0: (0x80003000,),
               B + 0x8f8: (0, 0x10000000), B + 0x8c4: (2,),
               # Channel 1 (histogram): destination, count, descriptor, arm.
               B + 0x704: (0x200,), B + 0x98c: (0, 1), B + 0x9f4: (0, 1),
               B + 0x9a0: (0x80403000, 0x80003000), B + 0x9f8: (0, 0x10000000),
               B + 0x9c4: (2,), B + 0x984: (0, 1)}
    if address == B + 0x700:
        ram_span(value, 1024)
        _store(device, core, address, value)
        s['config_stores'] += 1
        return True
    if address in allowed:
        if value not in allowed[address]:
            raise UnsupportedDevice('Producer control word is outside its stock values')
        if xm1 and address in (B + 0x8f4, B + 0x9f4) and value == 1 and r.get(address - 0x68, 0) != 0:
            raise UnsupportedDevice('Producer acknowledgment requires its cleared channel flag')
        _store(device, core, address, value)
        if xm1 and address in (B + 0x8f4, B + 0x9f4) and value == 1:
            s.setdefault('control_ack_due', {})[address] = device.board.virtual_ticks + device.ack_ticks
        s['control_stores'] = s.get('control_stores', 0) + 1
        return True
    if not xm1 and is_extra_word(device, address):
        # Retained 32-bit configuration the strict model has not catalogued
        # (global crop/window words etc.); stock setters mask their fields.
        old = r.get(address, 0)
        r[address] = value
        device.writes.append(dict(cpu=core, address=hex(address), value=hex(value), old=hex(old)))
        del device.writes[:-256]
        s['config_stores'] += 1
        s['extra_stores'] = s.get('extra_stores', 0) + 1
        return True
    if address in device.ALL_CONFIG_MASKS:
        if not device._config_valid(address, value):
            raise UnsupportedDevice('Producer configuration store has unknown bits/domain')
        _store(device, core, address, value)
        s['config_stores'] += 1
        return True
    if address == device.GLOBAL:
        if value == 0:
            if device.stop_profile is None:
                raise UnsupportedDevice('Producer stop acknowledgment needs a stop profile')
            s['stop_due'] = device.board.virtual_ticks + device.stop_ticks
            _store(device, core, address, 0)
        elif (device._global_valid(value) if xm1 else type(value) is int and 0 <= value <= 0x1fffffff):
            _store(device, core, address, value)
        else:
            raise UnsupportedDevice('Producer global control has unknown bits')
        s['config_stores'] += 1
        return True
    return False


def access(device, operation, core, address, size, value=None):
    s = device.stream
    if s is None:
        return False
    if _rotation_access(device, operation, core, address, value):
        return True
    ack = tuple(device.BASE + o for o in ACK_OFFSETS)
    if address in ack and s['state'] == 'delivered':
        return _ack_access(device, operation, core, address, size, value)
    return _register_file(device, operation, core, address, value)


def _ack_access(device, operation, core, address, size, value):
    s = device.stream
    if core != 0 or size != 4:
        raise UnsupportedDevice('Stream statistics ACK requires its WORD owner')
    channel, step = s['ack_channel'], s['ack_step']
    clear, flag = device.BASE + 0x88c + channel * 0x100, device.BASE + 0x8f4 + channel * 0x100
    word = device.registers[address] if operation == 'read' else value
    if type(word) is not int or word not in (0, 1):
        raise UnsupportedDevice('Stream statistics ACK word is not 0/1')
    if operation == 'write' and step == 0 and address == clear and word == 0:
        next_step = 1
    elif operation == 'write' and step == 1 and address == flag and word == 1:
        next_step = 2
    elif operation == 'read' and step == 2 and address == clear and word == 0:
        next_step = 3
    elif operation == 'read' and step == 3 and address == flag:
        next_step = 2 if word == 1 else 0
    else:
        raise UnsupportedDevice('Stream statistics require the next whole paired ACK access')
    now = device.board.virtual_ticks
    s['ack_step'] = next_step
    if operation == 'write':
        _store(device, core, address, word)
        if step == 1:
            s['ack_due_tick'] = now + device.ack_ticks
    if operation == 'read' and address == flag and word == 0:
        s['ack_channel'] += 1
        if s['ack_channel'] == 2:
            s.update(state='idle', ack_channel=0, ack_step=0, ack_due_tick=None, acknowledged_tick=now)
            s['frames_acknowledged'] += 1
    return True


