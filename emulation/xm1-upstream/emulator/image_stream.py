"""Continuous post-ISP image outputs for the opt-in sensor stream.

Generalizes the finite two-pair/AF-pair image lifetimes of
``image_work_devices`` to a repeating frame, only while the host selected the
stream (``emulator/testbed/STREAM.md``). While streaming, the image banks are
a permissive register file (the finite closed-reset/stop/AF-preview ledgers
are frozen and not audited): each word is checked against its stock mask,
and the stock handshakes keep their meaning:

* control (0x00): write 0 stops the bank, bit0 reads back set after the stop
  delay; 0x10001 arms it;
* D4: 0x33FF1 (stop acknowledgment) and 1 (event acknowledgment);
  D8: registration; configuration/passive words: stock masks only.

Per source0 edge, with both banks armed (control 0x10001): the
firmware-programmed destination/geometry tuple is latched, after a declared
delay the scene's YYCbCr (or the native analytic scene) is written there,
IRQ 0x2C/0x2D rise, and the D4=1 ACK completes each channel. Later tuple
stores do not affect a latched frame.
"""
import hashlib
from enum import Enum

from .hardware import UnsupportedDevice
from .virtual_source import FrameIndex, ram_span, xm1_source_tick

PROFILE = 'declared-testbed-continuous-image-stream-unmeasured'
IRQS = (0x2c, 0x2d)


class ImageStreamProfile(str, Enum):
    XM1 = 'xm1-150-declared-continuous-postisp-scene-unmeasured'


def enable(device, *, data_delay_ticks=50000, profile: ImageStreamProfile | None = None):
    if device.stream is not None:
        raise UnsupportedDevice('Continuous image stream is already enabled')
    if type(data_delay_ticks) is not int or data_delay_ticks < 1:
        raise UnsupportedDevice('Image data delay must be a positive integer')
    if profile is not None and type(profile) is not ImageStreamProfile:
        raise ValueError('Unknown typed image stream profile')
    selected = PROFILE if profile is None else profile
    device.stream = dict(profile=selected, data_delay_ticks=data_delay_ticks, state='idle',
        frame=None, outputs=None, due_tick=None, acked=[False, False], stop_due=[None, None],
        frames_latched=0, frames_delivered=0, frames_acknowledged=0, config_stores=0,
        skipped_busy=0, skipped_unarmed=0, skipped_route=0, bytes_delivered=0, last=None,
        source_tick=None, source_index=FrameIndex(0))
    device.board.record('Continuous image stream enabled', profile=selected,
        data_delay_ticks=data_delay_ticks,
        limits='Scene YYCbCr bytes into firmware-latched destinations before IRQ 0x2C/0x2D; stock D4 ACK; unmeasured timing')


def disable(device):
    device.stream = None


def _armed_channels(device):
    return [c for c, base in enumerate(device.BASES) if device.registers[base] == 0x10001]


def read(device, core, channel, offset):
    address = device.BASES[channel] + offset
    value = device.registers[address]
    if offset == 0 and value not in (0, 1, 0x10001):
        raise UnsupportedDevice('Image-bank control has an unmodeled value')
    if offset in device.ALL_CONFIG_MASKS and not device._config_valid(offset, value) and value != 0:
        raise UnsupportedDevice('Image-bank configuration word has an unmodeled value')
    return value


def write(device, core, channel, offset, value):
    s = device.stream
    base = device.BASES[channel]
    address = base + offset
    now = device.board.virtual_ticks
    if offset == 0:
        if value not in (0, 0x10001):
            raise UnsupportedDevice('Image-bank control write is outside stop (0) / arm (0x10001)')
        if value == 0:
            s['stop_due'][channel] = now + device.stop_ticks
        else:
            s['stop_due'][channel] = None
    elif offset == 0xd4:
        if value == 1 and s['state'] == 'delivered' and not s['acked'][channel]:
            s['acked'][channel] = True
            if all(s['acked']):
                s['state'] = 'idle'
                s['frames_acknowledged'] += 1
        elif value not in (1, device.STOP_ACK):
            raise UnsupportedDevice('Image-bank D4 write is outside the stock event/stop acknowledgments')
    elif offset == 0xd8:
        if value != 1:
            raise UnsupportedDevice('Image-bank D8 registration must write 1')
    elif offset in device.ALL_CONFIG_MASKS:
        if not device._config_valid(offset, value):
            raise UnsupportedDevice('Image-bank configuration store has unknown bits/domain')
        s['config_stores'] += 1
    else:
        raise UnsupportedDevice('Unknown image-bank offset')
    device.registers[address] = value
    device.writes.append(dict(cpu=core, address=hex(address), value=hex(value)))
    del device.writes[:-256]


def latch_frame(device, frame, tick):
    s = device.stream
    if s is None:
        return False
    if s['state'] != 'idle':
        s['skipped_busy'] += 1
        return False
    channels = _armed_channels(device)
    if not channels:
        s['skipped_unarmed'] += 1
        return False
    try:
        outputs = [device._data_tuple(c) for c in channels]
    except UnsupportedDevice:
        if s['profile'] is ImageStreamProfile.XM1:
            raise
        s['skipped_unarmed'] += 1   # tuple mid-update / invalid geometry: no transfer
        return False
    if (len(outputs) == 2 and outputs[0]['physical_begin'] < outputs[1]['physical_end']
            and outputs[1]['physical_begin'] < outputs[0]['physical_end']):
        if s['profile'] is ImageStreamProfile.XM1:
            raise UnsupportedDevice('Armed image-bank DMA destinations overlap')
        s['skipped_unarmed'] += 1
        return False
    if len(outputs) == 1:
        s['single_bank_frames'] = s.get('single_bank_frames', 0) + 1
    # Only the latched banks owe the stock D4=1 acknowledgment.
    s.update(state='latched', frame=dict(frame, latched_tick=tick), outputs=outputs,
             due_tick=tick + s['data_delay_ticks'], acked=[c not in channels for c in range(2)])
    s['frames_latched'] += 1
    return True


def _latch_xm1_clock(device, board):
    tick = xm1_source_tick(board)
    stream = device.stream
    if tick is None or tick == stream['source_tick']:
        return
    stream['source_tick'] = tick
    stream['source_index'] = FrameIndex(stream['source_index'] + 1)
    latch_frame(device, {'index': stream['source_index'], 'tick': tick}, tick)


def tick(device, board):
    s = device.stream
    if s is None:
        return
    if s['profile'] is ImageStreamProfile.XM1:
        _latch_xm1_clock(device, board)
    for channel in (0, 1):
        due = s['stop_due'][channel]
        if due is not None and board.virtual_ticks >= due:
            device.registers[device.BASES[channel]] = 1
            s['stop_due'][channel] = None
    if s['state'] != 'latched' or board.virtual_ticks < s['due_tick']:
        return
    for item in s['outputs']:
        try:
            route = device._data_route(item['channel'])
        except UnsupportedDevice:
            route = None
        if route != item['route'] or device.registers[device.BASES[item['channel']]] != 0x10001:
            # Bank stopped or IRQ route withdrawn mid-frame: no transfer.
            s['skipped_route'] += 1
            s['state'] = 'idle'
            return
    host = getattr(board, 'sensor_stream', None)
    payloads = []
    for item in s['outputs']:
        data = host.image(s['frame'], item) if host is not None else None
        if data is None:
            from x70_native._native import virtual_scene_yycbcr
            data = virtual_scene_yycbcr(item['width'], item['height'], item['stride_bytes'],
                s['frame']['index'], item['source_width'], item['source_height'],
                0, 0, item['source_width'], item['source_height'])
        if len(data) != item['bytes']:
            raise UnsupportedDevice('Stream image payload length differs from the latched span')
        ram_span(int(item['address'], 16), item['bytes'])
        payloads.append(data)
    for item, data in zip(s['outputs'], payloads):
        board.write_memory(int(item['address'], 16), data)
        s['bytes_delivered'] += len(data)
    s['last'] = dict(frame=s['frame']['index'], sha256=[hashlib.sha256(d).hexdigest() for d in payloads],
                     addresses=[i['address'] for i in s['outputs']])
    for item in s['outputs']:
        board.queue_irq(item['route'], IRQS[item['channel']])
    s['state'] = 'delivered'
    s['frames_delivered'] += 1


def due(device):
    s = device.stream
    if s is None:
        return None
    times = [t for t in s['stop_due'] if t is not None]
    if s['state'] == 'latched':
        times.append(s['due_tick'])
    return min(times) if times else None
