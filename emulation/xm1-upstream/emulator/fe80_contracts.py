"""Immutable firmware descriptor/program inputs to the shared FE80 transport."""
from dataclasses import dataclass
import hashlib
import struct
from .boards.control_bus_parameters import FirmwareView


@dataclass(frozen=True)
class FE80Program:
    descriptor: FirmwareView
    program: FirmwareView
    period: int
    setup_phase: str = 'routed'

    def __post_init__(self):
        if (not isinstance(self.descriptor,FirmwareView) or not isinstance(self.program,FirmwareView)
                or self.descriptor.sampling != 'immutable-source' or self.program.sampling != 'immutable-source'
                or (self.descriptor.firmware_id,self.descriptor.source_sha256) !=
                   (self.program.firmware_id,self.program.source_sha256)
                or len(self.descriptor.data) != 16 or self.descriptor.address & 3
                or type(self.period) is not int or not 1 <= self.period <= 0x10000
                or self.setup_phase not in ('routed','disabled')
                or not self.program.data or self.program.address & 3):
            raise ValueError('Unadmitted FE80 firmware program contract')
        if struct.unpack('<4I',self.descriptor.data) != (0x03000000,self.program.address,len(self.program.data),0):
            raise ValueError('FE80 descriptor differs from its exact opaque program view')
        if any(hashlib.sha256(view.data).hexdigest() != view.sha256
               for view in (self.descriptor,self.program)):
            raise ValueError('FE80 firmware view hash differs from bytes')
