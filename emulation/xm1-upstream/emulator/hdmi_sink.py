"""Opt-in connected HDMI sink for the testbed (host-selected, declared, unmeasured).

`HDMIStartup` (hdmi_devices.py) models only an absent cable. This module adds the
*connected* profile `declared-testbed-hdmi-sink-1080p-unmeasured`, which the host
selects explicitly with `HDMISink.install(board)`; it is never part of a stock
profile and no physical transmitter was measured. Every behavior below is derived
from what the stock driver (0x970460..0x972604) reads and writes, and is stated
as a declared fixture. Unmodeled registers raise `UnsupportedDevice` (strict) so
the missing behavior is found by the failure, not hidden.

Stock hot-plug path (corpus: research/arm, `tools/arm_corpus.py function ...`):

* `hdmi_task` (0x9726c4) registers power callback 0x9714fc. That attaches
  0x97150c to interrupt selector 0x5b (`0x919510`). Table 0xaeafe0 + 0x5b*12
  reads (source 9, priority 12, mode 0) in the REC snapshot, and the vector
  `0xfff00008 + 9*4` holds 0x97150c. The IRQ handler sends message 0x5500.
  `plug()` raises source 9 on CPU0 on the HPD rising edge. Whether the physical
  pin is edge or level is unknown; this is an edge fixture.
* 0x5500 (0x97264c) sets DDC enables (0xfff77050 bit3, 0xfff77040 bit0) and arms
  poll 0x971600 (kernel timer). The poll reads 0xfff7637c bit6 (HPD) and GPIO
  0xfff89014 bit1 (BootControls; 0 in the idle snapshot) and posts 0x5501
  after three consistent polls, then 0x5504 steps.
* state1 (0x971944) needs 0xfff762dc bit0 and bit1 (declared "transmitter ready",
  set while HPD is high); otherwise it clears the DDC bus (0x972254, nine SCL
  pulses on 0xfff77084), retries three times, and gives up.
* state3 (0x970460) writes 0xfff76248=0xc6 (first block), then segment pointer
  0xfff76310 and offset 0xfff76314. The declared engine starts a 128-byte read
  on the offset write while 0xfff76248 & 6 == 6, fills the byte-per-word window
  0xfff77200..0xfff773fc (byte 126, the extension count, sits at 0xfff773f8) and
  sets 0xfff76250 bit2 after `edid_ticks`; bit1 would mean error. 0xfff76250
  clears bits written as 1 (the driver writes 0xff).
* state5 (0x971fc8) parses block 1 as CEA-861: first data block must be the video
  block (word 4 = tag/length, words 5.. = VICs), then it scans for the HDMI VSDB
  (`03 0c 00`). The firmware picks the first VIC of table 0xaffc1c the sink lists.
* state6 (0x972544) writes the transmitter, timing, InfoFrame packet words
  (0xfff7617c..; with an HDMI VSDB sink the 0x84-header variant overwrites the 0x82 AVI one,
  so the VIC slot reads 0) and audio registers, then notifies KEY_HDTV (0x19).
"""
from collections import deque

from .hardware import UnsupportedDevice
from .hdmi_devices import HDMIStartup

PROFILE = 'declared-testbed-hdmi-sink-1080p-unmeasured'
HPD_IRQ_SOURCE = 9
TX, DDC = 0xfff76000, 0xfff77000
HPD_STATUS = TX + 0x37c
READY_STATUS = TX + 0x2dc
INT_STATUS, INT_STATUS2 = TX + 0x250, TX + 0x254
EDID_CONTROL, EDID_SEGMENT, EDID_OFFSET = TX + 0x248, TX + 0x310, TX + 0x314
EDID_WINDOW, EDID_WINDOW_END = DDC + 0x200, DDC + 0x400
DDC_BUS_CONTROL = DDC + 0x84
# Display bridge shadow commit block: 0x960174 reads word 0 and fills the 0xc9ba10 shadow,
# 0x9600fc stores its seven words (cfgdata-derived) during every HDMI image-engine switch.
BRIDGE = 0xfff68000
BRIDGE_WORDS = tuple(BRIDGE + 4 * i for i in range(7))

# Addresses the stock driver references (all literal references in research/arm functions
# 0x9660xx..0x9726xx that touch 0xfff76000..0xfff77fff), held as plain 32-bit registers
# that read back the last written value. Reads of registers the driver only writes are
# accepted for the same reason, as a documented simplification.
STORED_REGISTERS = frozenset(
    [TX + o for o in (
        0x000, 0x004, 0x008, 0x00c, 0x01c, 0x020, 0x024, 0x028, 0x02c, 0x030, 0x034, 0x040,
        0x044, 0x048, 0x04c, 0x050, 0x054, 0x058, 0x0c0, 0x0c4, 0x0c8, 0x0cc, 0x0d0, 0x0d4,
        0x0d8, 0x0dc, 0x0e0, 0x0e4, 0x0e8, 0x0f4, 0x0f8, 0x0fc, 0x104, 0x108, 0x114,
        0x248, 0x254, 0x2bc, 0x310, 0x314)]
    + [TX + o for o in range(0x17c, 0x1fc, 4)]           # AVI/audio InfoFrame packet words
    + list(BRIDGE_WORDS)
    + [DDC + o for o in (0x018, 0x024, 0x028, 0x02c, 0x034, 0x038, 0x03c, 0x040, 0x044,
                         0x050, 0x058, 0x084)])
READ_ONLY = {HPD_STATUS: 'HPD status (bit6)', READY_STATUS: 'transmitter ready (bits0-1)'}

# CEA short video descriptors: 21 entries like the recorded negotiation (count only; the
# real capture card's list was not retained), 1080p60 first and native.
DEFAULT_VICS = (16, 5, 4, 31, 20, 19, 32, 33, 34, 3, 2, 1, 7, 6, 18, 17, 22, 21, 15, 14, 29)


def _checksum(block):
    block = bytearray(block[:127])
    return bytes(block) + bytes([(-sum(block)) & 0xff])


def build_edid(vics=DEFAULT_VICS, *, name='TESTBED HDMI'):
    """Deterministic 256-byte EDID 1.3 + CEA-861 rev3 extension (1080p60 preferred)."""
    base = bytearray(128)
    base[0:8] = bytes([0, 255, 255, 255, 255, 255, 255, 0])
    base[8:10] = ((ord('T') - 64) << 10 | (ord('S') - 64) << 5 | (ord('T') - 64)).to_bytes(2, 'big')
    base[10:12] = (0x0001).to_bytes(2, 'little')
    base[12:16] = (1).to_bytes(4, 'little')
    base[16], base[17] = 1, 26
    base[18], base[19] = 1, 3
    base[20], base[21], base[22], base[23], base[24] = 0x80, 0x3c, 0x22, 0x78, 0x0a
    base[25:35] = bytes.fromhex('eee591a3544c99260f50')           # sRGB-like chromaticity
    base[38:54] = bytes([1, 1] * 8)                              # unused standard timings
    dtd_1080p60 = bytes.fromhex('023a801871382d40582c4500c48e2100001e')
    base[54:72] = dtd_1080p60
    base[72:90] = bytes([0, 0, 0, 0xfc, 0]) + name.encode('ascii')[:13].ljust(13, b' ')[:12] + b'\n'
    base[90:108] = bytes([0, 0, 0, 0xfd, 0, 50, 76, 30, 80, 20, 0, 10, 32, 32, 32, 32, 32, 32])
    base[108:126] = bytes([0, 0, 0, 0x10, 0]) + bytes(13)
    base[126] = 1
    base = _checksum(base)
    video = bytes([0x40 | len(vics)]) + bytes(v | 0x80 if i == 0 else v for i, v in enumerate(vics))
    audio = bytes([0x23, 0x09, 0x07, 0x07])
    vsdb = bytes([0x65, 0x03, 0x0c, 0x00, 0x10, 0x00])
    speakers = bytes([0x83, 0x01, 0x00, 0x00])
    blocks = video + audio + vsdb + speakers
    ext = bytearray(128)
    ext[0], ext[1], ext[2], ext[3] = 2, 3, 4 + len(blocks), 0x71
    ext[4:4 + len(blocks)] = blocks
    ext[4 + len(blocks):4 + len(blocks) + 18] = dtd_1080p60
    return base + _checksum(ext)


class HDMISink(HDMIStartup):
    """Connected-sink superset of HDMIStartup; install replaces the stock startup device."""

    def __init__(self, board, *, hpd=False, edid=None, edid_ticks=2000, strict=True, log_limit=4000):
        if not isinstance(strict, bool) or type(edid_ticks) is not int or edid_ticks < 1:
            raise ValueError('HDMI sink options are strict:boolean and edid_ticks:positive int')
        super().__init__(board, hpd=bool(hpd))
        self.profile, self.strict = PROFILE, strict
        self.edid = bytes(build_edid() if edid is None else edid)
        if len(self.edid) % 128 or not self.edid:
            raise ValueError('EDID must be a whole number of 128-byte blocks')
        self.edid_ticks = edid_ticks
        self.registers = {}                           # startup bank folded into `stored`
        self.stored = {a: 0 for a in STORED_REGISTERS}
        self.window = bytearray(128)
        self.int_status = 0
        self.edid_due = None
        self.log, self.counts, self.unknown = deque(maxlen=log_limit), {}, []
        self.edid_reads, self.plugs, self.unplugs = [], 0, 0
        self.board.record('HDMI connected-sink profile', profile=PROFILE, hpd=self.hpd,
                          edid_bytes=len(self.edid), strict=strict,
                          provenance='Register roles inferred from stock driver 0x970460..0x972604; see emulator/hdmi_sink.py',
                          limits='Declared fixture: no measured TX behavior, EDID, lock timing, IRQ polarity or transmitted pixels')

    @classmethod
    def install(cls, board, **options):
        """Replace the board's HDMIStartup (carrying its registers) with a connected-sink model."""
        old = next((d for d in board.devices if type(d) is HDMIStartup), None)
        if old is None:
            existing = next((d for d in board.devices if isinstance(d, cls)), None)
            if existing is not None:
                return existing
            raise ValueError('Board has no stock HDMIStartup device to replace')
        sink = cls(board, hpd=old.hpd, **options)
        sink.stored.update({a: v for a, v in old.registers.items() if a in sink.stored})
        sink.writes = list(old.writes)                 # startup selector/enable history
        board.devices[board.devices.index(old)] = sink
        return sink

    # Host actions
    def plug(self):
        """Raise HPD and the stock HPD interrupt source (CPU0)."""
        if self.hpd:
            return
        self.hpd = True
        self.plugs += 1
        self.board.record('HDMI sink plugged', hpd=True, irq_source=HPD_IRQ_SOURCE)
        self.board.queue_irq(0, HPD_IRQ_SOURCE)

    def unplug(self):
        """Drop HPD; the driver's own poll (0x971600) notices it after three polls."""
        if not self.hpd:
            return
        self.hpd = False
        self.unplugs += 1
        self.int_status = 0
        self.edid_due = None
        self.board.record('HDMI sink unplugged', hpd=False)

    # Device interface
    def _kind(self, address, size):
        if size != 4 or address % 4:
            return None
        if address in self.stored or address in READ_ONLY:
            return 'reg'
        if address in (INT_STATUS,):
            return 'status'
        if EDID_WINDOW <= address < EDID_WINDOW_END:
            return 'edid'
        return None

    def contains(self, address, size):
        # Claim the whole TX/DDC aperture and the bridge words so unknown accesses are logged and rejected here,
        # rather than failing as 'Unknown MMIO' with no register context.
        return TX <= address < DDC + 0x1000 or address in BRIDGE_WORDS

    def _note(self, kind, address, size, value, note=None):
        self.log.append((kind, self.board.instructions, self.board.virtual_ticks, address, size, value))
        key = (kind, address)
        self.counts[key] = self.counts.get(key, 0) + 1

    def _reject(self, kind, address, size, value, why):
        entry = {'kind': kind, 'address': hex(address), 'size': size, 'value': hex(value),
                 'instructions': self.board.instructions, 'why': why}
        self.unknown.append(entry)
        self.board.record('HDMI sink unmodeled access', **entry)
        if self.strict:
            raise UnsupportedDevice(f'Unmodeled HDMI sink {kind} {address:#x}/{size}: {why}')
        return 0

    def read(self, core, address, size):
        kind = self._kind(address, size)
        if kind is None:
            return self._reject('read', address, size, 0, 'register not referenced by the stock driver or bad access size')
        if address == HPD_STATUS:
            value = int(self.hpd) << 6
        elif address == READY_STATUS:
            value = 3 if self.hpd else 0
        elif address == INT_STATUS:
            value = self.int_status
        elif kind == 'edid':
            value = self.window[(address - EDID_WINDOW) // 4]  # one byte per 32-bit word
        else:
            value = self.stored[address]
        self._note('r', address, size, value)
        return value

    def write(self, core, address, size, value):
        value &= 0xffffffff
        kind = self._kind(address, size)
        if kind is None:
            return self._reject('write', address, size, value, 'register not referenced by the stock driver or bad access size')
        if address in READ_ONLY or kind == 'edid':
            return self._reject('write', address, size, value, 'read-only status/EDID window')
        self._note('w', address, size, value)
        if address == INT_STATUS:
            self.int_status &= ~value & 0xffffffff     # write-one-clear (driver writes 0xff)
            return
        self.stored[address] = value
        if address == EDID_OFFSET and (self.stored[EDID_CONTROL] & 6) == 6:
            self._start_edid_read()

    def _start_edid_read(self):
        segment, offset = self.stored[EDID_SEGMENT], self.stored[EDID_OFFSET]
        start = segment * 256 + offset
        record = {'segment': segment, 'offset': offset, 'start_tick': self.board.virtual_ticks,
                  'ok': offset in (0, 0x80) and start + 128 <= len(self.edid) and segment < 2}
        self.edid_reads.append(record)
        self.pending_read = record
        self.edid_due = self.board.virtual_ticks + self.edid_ticks

    def tick(self, board):
        if self.edid_due is not None and board.virtual_ticks >= self.edid_due:
            self.edid_due = None
            record = self.pending_read
            if record['ok'] and self.hpd:
                start = record['segment'] * 256 + record['offset']
                self.window[:] = self.edid[start:start + 128]
                self.int_status |= 4
            else:
                self.int_status |= 2
            record['done_tick'] = board.virtual_ticks

    def next_event_tick(self, now):
        return self.edid_due

    # Decoded state for hosts/tests
    def infoframe_words(self):
        """Raw words of the packet registers 0xfff7617c.. (header, payload, VIC slot at +0x20)."""
        return [self.stored[TX + o] for o in range(0x17c, 0x1fc, 4)]

    def snapshot(self):
        state = {'writes': list(self.writes)}
        state.update(name='HDMISink', profile=self.profile, hpd=self.hpd, strict=self.strict,
                     plugs=self.plugs, unplugs=self.unplugs, int_status=hex(self.int_status),
                     edid_reads=list(self.edid_reads), infoframe=[hex(v) for v in self.infoframe_words()[:10]],
                     stored={hex(a): hex(v) for a, v in sorted(self.stored.items()) if v},
                     access_counts={f'{k}:{hex(a)}': n for (k, a), n in sorted(self.counts.items())},
                     unknown=list(self.unknown))
        state['limits'] = [
            'Declared connected-sink fixture; no measured transmitter, EDID, lock timing or IRQ polarity.',
            'Transmitter ready (0xfff762dc bits0-1) is simply set while HPD is high.',
            'EDID reads complete after a fixed virtual delay; interrupt status bit2 means done, bit1 error.',
            'HPD interrupt is an edge fixture on source 9, CPU0; no re-arm on a persistent level.',
            'Registers are write-back storage, except HPD/ready/status/EDID window.',
            'No HDMI pixel transmission: the host captures display image instance 0 from RAM.']
        return state
