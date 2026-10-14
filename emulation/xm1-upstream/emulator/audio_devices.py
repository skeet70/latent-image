"""Retained X-M1 serial-audio words decoded from 006e64c8..006e6d8c."""
from enum import IntEnum
import hashlib
from .hardware import UnsupportedDevice


class AudioRegister(IntEnum):
    CONTROL = 0x04
    FORMAT = 0x08
    CHANNEL_ENABLE = 0x0c
    IRQ_ENABLE = 0x40
    STATUS = 0x50
    SAMPLE_WIDTH = 0x100


REGISTER_MASKS = {
    AudioRegister.CONTROL: 0x01030001,
    AudioRegister.FORMAT: 0x00038003,
    AudioRegister.CHANNEL_ENABLE: 0x00010001,
    AudioRegister.IRQ_ENABLE: 1,
    AudioRegister.STATUS: 0,
    AudioRegister.SAMPLE_WIDTH: 3,
}


class AudioSerialControl:
    BASES = (0xfffa0000, 0xfffa1000)

    def __init__(self, board):
        self.board = board
        self.registers = [{register: 0 for register in AudioRegister} for _ in self.BASES]
        self.writes = [0, 0]
        self.dma_owner = None
        self.transmitted_bytes = [0, 0]
        self.last_transmission = None

    @classmethod
    def contains(cls, address, size):
        return size > 0 and any(base <= address and address + size <= base + 0x1000
                                for base in cls.BASES)

    def mmio_ranges(self):
        return tuple((base, base + 0x1000) for base in self.BASES)

    @classmethod
    def _location(cls, address, size):
        if size != 4 or address & 3:
            raise UnsupportedDevice(f'Serial-audio control requires aligned words: {address:#x}/{size}')
        for bank, base in enumerate(cls.BASES):
            if address - base in REGISTER_MASKS:
                return bank, AudioRegister(address - base)
        raise UnsupportedDevice(f'Unmodeled serial-audio register {address:#x}')

    def read(self, cpu_id, address, size):
        bank, register = self._location(address, size)
        return self.registers[bank][register]

    def write(self, cpu_id, address, size, value):
        bank, register = self._location(address, size)
        if register == AudioRegister.STATUS:
            raise UnsupportedDevice('Serial-audio FIFO status is read-only')
        if not 0 <= value <= 0xffffffff or value & ~REGISTER_MASKS[register]:
            raise UnsupportedDevice(f'Unmodeled serial-audio control bits {address:#x}={value:#x}')
        if register == AudioRegister.CONTROL and value & 0x30000 == 0x30000:
            raise UnsupportedDevice('Unobserved serial-audio mode field 3')
        if register == AudioRegister.FORMAT and value & 3 == 3:
            raise UnsupportedDevice('X-M1 serial-audio format field 3 is unsupported')
        self.registers[bank][register] = value
        self.writes[bank] += 1
        self.board.record('Serial-audio control write', cpu=cpu_id, bank=bank,
                          address=hex(address), value=hex(value),
                          provenance='X-M1 stock control RMW retained; no sample or transfer completion generated')
        if self.dma_owner is not None:
            self.dma_owner.wake_audio_channels()

    def can_transmit(self, destination):
        for bank, base in enumerate(self.BASES):
            if destination == base + 0x10:
                return bool(self.registers[bank][AudioRegister.FORMAT] & 0x20000)
        raise UnsupportedDevice(f'Unsupported serial-audio DMA endpoint {destination:#x}')

    def transmit_dma(self, destination, payload):
        if not self.can_transmit(destination):
            raise UnsupportedDevice('Serial-audio DMA reached a disabled transmitter')
        bank = (destination - self.BASES[0]) // 0x1000
        self.transmitted_bytes[bank] += len(payload)
        self.last_transmission = dict(bank=bank, bytes=len(payload),
                                      sha256=hashlib.sha256(payload).hexdigest())
        self.board.record('Serial-audio stream transmitted', **self.last_transmission,
                          limits='Atomic virtual byte-stream sink; no physical codec or host speaker')

    def snapshot(self):
        return {'device': type(self).__name__,
                'banks': [{'base': hex(base),
                           'registers': {hex(register): hex(value) for register, value in self.registers[bank].items()},
                           'writes': self.writes[bank]}
                          for bank, base in enumerate(self.BASES)],
                'transmitted_bytes': list(self.transmitted_bytes),
                'last_transmission': self.last_transmission,
                'limits': ['Initial control words zero are explicit simulation fixtures.',
                           '+8 format/enable fields, +0xc channels, +0x40 IRQ enable and +0x100 sample width follow X-M1 stock setters/getters.',
                           'DMA transmits a byte stream to a virtual sink; physical timing, sample packing and codec output are unmeasured.']}
