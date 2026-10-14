"""Continuous FE80 preview-processing jobs for the opt-in sensor stream.

The strict FE80 model accepts the setup handshakes, one program upload and
the exact first AF preview start, with an unknown numerical result. While
the stream is enabled the controller is instead a permissive register file
(every FE80 page word keeps what the firmware wrote), with the stock
handshake status words and completions:

* ``D840`` write 0 clears ``D84C``; ``D844`` write 1 sets ``D84C`` after the
  handshake delay (reset and enable);
* ``C1C0`` write 1 starts the opaque program upload; ``C000`` reads 0x10 when
  it completes and the stock write of 0x10 acknowledges it;
* ``C100`` write 1 starts a preview job. After a declared delay the host's
  14 U32 contrast values are written to the output pointer found in the
  firmware-programmed parameter block (descriptor ``C108`` -> entry 0 ->
  word 1), ``D804`` reads 0 (no error) and physical IRQ 88 rises for the
  stock ``013372E8`` handler, which does its own retirement accesses.

The 14 values are a declared law (``SceneSource.fe80_values``): FE80's
numerical program and the meaning of its 14 channels are not recovered; the
stock consumer (``12BBE30``) stores them as numeric-history series 0 and
smooths them into series 1.
"""
import struct

from .hardware import UnsupportedDevice

PROFILE = 'declared-testbed-continuous-fe80-contrast-stream-unmeasured'
PAGES = ((0xfe80c000, 0x1000), (0xfe80d000, 0x1000), (0xfe318000, 0x1000), (0xfe3b0000, 0x1000))
IRQ = 88
HANDLER = 0x13372e8
CHANNELS = 14
UPLOAD_BYTES_PER_TICK = 256


def in_pages(address):
    return any(base <= address < base + size for base, size in PAGES)


def enable(device, *, job_delay_ticks=30000):
    if device.stream is not None:
        raise UnsupportedDevice('Continuous FE80 stream is already enabled')
    if type(job_delay_ticks) is not int or job_delay_ticks < 1:
        raise UnsupportedDevice('FE80 job delay must be a positive integer')
    device.stream = dict(profile=PROFILE, job_delay_ticks=job_delay_ticks, handshake_due=None,
                         upload_due=None, job_due=None, job=None, jobs_started=0, jobs_completed=0,
                         jobs_skipped=0, last=None, stores=0)
    device.board.record('Continuous FE80 stream enabled', profile=PROFILE, job_delay_ticks=job_delay_ticks,
                        limits='Declared 14-value contrast output and IRQ 88 per C100 start; opaque program not executed')


def disable(device):
    device.stream = None


def read(device, core, address):
    value = device.registers.get(address, 0)
    return value


def write(device, core, address, value):
    s = device.stream
    now = device.board.virtual_ticks
    r = device.registers
    if address == 0xfe80d840:
        r[0xfe80d84c] = 0
    elif address == 0xfe80d844 and value == 1:
        r[0xfe80d84c] = 0
        s['handshake_due'] = now + device.handshake_ticks
    elif address == 0xfe80c1c0 and value == 1:
        r[0xfe80c000] = 0
        s['upload_due'] = now + max(device.handshake_ticks, 74968 // UPLOAD_BYTES_PER_TICK)
    elif address == 0xfe80c000 and value == 0x10:
        r[address] = 0
        return
    elif address == 0xfe80c100 and value == 1:
        s['job_due'] = now + s['job_delay_ticks']
        s['jobs_started'] += 1
    r[address] = value
    s['stores'] += 1


def _output_pointer(device):
    r = device.registers
    descriptor = r.get(0xfe80c108, 0)
    raw = bytes(device.board.read_memory(descriptor, 16, cpu_id=0))
    flags, pointer, size, _ = struct.unpack('<4I', raw)
    if flags != 0x03000207 or size < 8:
        raise UnsupportedDevice('FE80 job descriptor entry 0 is not the stock parameter block')
    block = bytes(device.board.read_memory(pointer, 8, cpu_id=0))
    return struct.unpack('<2I', block)[1]


def _route(device):
    banks = device.board.interrupts.banks
    targets = [i for i, b in enumerate(banks) if b.enabled & (1 << IRQ)]
    if len(targets) != 1:
        return None
    bank = banks[targets[0]]
    if (bank.config.get(0x400 + 4 * IRQ) != HANDLER or bank.pending & (1 << IRQ)
            or bank.active == IRQ or bank.injected == IRQ):
        return None
    return targets[0]


def tick(device, board):
    s = device.stream
    if s is None:
        return
    now = board.virtual_ticks
    if s['handshake_due'] is not None and now >= s['handshake_due']:
        device.registers[0xfe80d84c] = 1
        s['handshake_due'] = None
    if s['upload_due'] is not None and now >= s['upload_due']:
        device.registers[0xfe80c000] = 0x10
        s['upload_due'] = None
    if s['job_due'] is not None and now >= s['job_due']:
        s['job_due'] = None
        route = _route(device)
        if route is None:
            s['jobs_skipped'] += 1      # stopped/withdrawn before completion: no transfer
            return
        host = getattr(board, 'sensor_stream', None)
        values = host.fe80_values(CHANNELS) if host is not None else None
        if values is None or len(values) != CHANNELS:
            raise UnsupportedDevice('FE80 stream needs 14 contrast values from the scene host')
        output = _output_pointer(device)
        board.write_memory(output, struct.pack('<%dI' % CHANNELS, *values), cpu_id=0)
        device.registers[0xfe80d804] = 0
        s['jobs_completed'] += 1
        s['last'] = dict(tick=now, output=hex(output), values=list(values))
        board.queue_irq(route, IRQ)


def due(device):
    s = device.stream
    if s is None:
        return None
    times = [t for t in (s['handshake_due'], s['upload_due'], s['job_due']) if t is not None]
    return min(times) if times else None
