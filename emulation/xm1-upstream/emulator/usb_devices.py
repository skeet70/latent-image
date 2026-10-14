"""Bounded stock USB startup with explicit external cable-presence input.

The initial no-cable driver programs PHY words and halfword core controls.
These writes have no observed ready/complete poll and create no USB traffic,
endpoint data or IRQ. Unknown registers/commands remain unsupported.
"""
from .hardware import UnsupportedDevice


class USBControl:
    PHY=0xfff55000
    CORE=0xfff54000
    WORD_OFFSETS={0x100,0x104,0x108}
    HALFWORD_REGISTERS={0xfff54008,0xfff54090,0xfff54094,0xfff54098,
                        0xfff540ac,0xfff552fc}

    _endpoint = None

    def __init__(self,board,power, *, endpoint_contract=None):
        if getattr(power,'usb_control',None) is not None:
            raise ValueError('USB status already belongs to another USB control device')
        if not isinstance(power.usb_connected,bool):
            raise ValueError('USB presence fixture must be boolean')
        self.board,self.power=board,power
        self.registers={**{self.PHY+offset:0 for offset in self.WORD_OFFSETS},
                        **{address:0 for address in self.HALFWORD_REGISTERS}}
        self.started=False
        self.configured=False
        self.reset_pulses=0
        self.writes=[]
        power.usb_control=self
        board.record('USB controller input/reset fixture',usb_connected=power.usb_connected,
            status_bit31_source='existing explicit PowerDevices.usb_connected input',
            register_reset='unmeasured zero configuration/core words',
            provenance='stock10A8FB0 init,10A9688 presence,10AA48C stop; no external USB capture',
            limits='register-only no-cable startup; no USB enumeration/transfer/completion/IRQ inferred')
        if endpoint_contract is not None:
            from .usb_endpoint import USBEndpointEngine
            self._endpoint = USBEndpointEngine(board, endpoint_contract)

    def contains(self,address,size):
        return ((self._endpoint is not None and self._endpoint.contains(address,size)) or
                (size==4 and address-self.PHY in self.WORD_OFFSETS) or
                (size==2 and address in self.HALFWORD_REGISTERS))

    def mmio_ranges(self):
        """Superset of the start addresses contains() accepts (RustBoard._mmio_device index)."""
        return (*((self.PHY+offset, self.PHY+offset+1) for offset in self.WORD_OFFSETS),
                *((address, address+1) for address in self.HALFWORD_REGISTERS),
                *(self._endpoint.mmio_ranges() if self._endpoint is not None else ()))

    def read(self,core,address,size):
        if self._endpoint is not None and self._endpoint.contains(address,size):
            return self._endpoint.read(core,address,size)
        if not self.contains(address,size):
            raise UnsupportedDevice(f'Unknown USB control read {address:#x}/{size}')
        if address==self.PHY+0x104:
            return self.registers[address] | (int(self.power.usb_connected)<<31)
        if address not in (self.PHY+0x108,0xfff54008,0xfff54090,
                            0xfff54094,0xfff54098,0xfff552fc):
            raise UnsupportedDevice(f'Unproven USB control read {address:#x}/{size}')
        return self.registers[address]

    def write(self,core,address,size,value):
        if self._endpoint is not None and self._endpoint.contains(address,size):
            return self._endpoint.write(core,address,size,value)
        if not self.contains(address,size):
            raise UnsupportedDevice(f'Unknown USB control write {address:#x}/{size}')
        # Unicorn's STRH callback exposes the entire source register (e.g.
        # MVN yieldsFFFF3FFF), while the actual bus write carries low16 bits.
        if not 0<=value<=0xffffffff:
            raise UnsupportedDevice('USB control source register exceeds32 bits')
        value &= (1<<(size*8))-1
        if address==self.PHY+0x100:
            if value!=1:raise UnsupportedDevice(f'Unknown USB PHY start {value:#x}')
            self.started=True
        elif address==self.PHY+0x104:
            if not self.started or value!=0x1ecbc7:
                raise UnsupportedDevice(f'Unknown USB PHY configuration/state {value:#x}')
            self.configured=True
        elif address==self.PHY+0x108:
            if value not in (0,0x10000):
                raise UnsupportedDevice(f'Unknown USB PHY reset control {value:#x}')
            if value==0x10000:self.reset_pulses+=1
        else:
            if not self.configured:
                raise UnsupportedDevice('USB core control precedes the known PHY configuration')
            known={0xfff54008:{0,2},0xfff54090:{0,1},0xfff54094:{0,0xc0},
                   0xfff54098:{0,1},0xfff540ac:{0x3fff},0xfff552fc:{0,1}}
            if value not in known[address]:
                raise UnsupportedDevice(f'Unknown USB core command {address:#x}={value:#x}')
        self.registers[address]=value
        event={'cpu':core,'address':hex(address),'size':size,'value':hex(value)}
        self.writes.append(event)
        self.board.record('USB controller configuration write',**event)

    def snapshot(self):
        state = {'name':'USBControl','usb_connected':self.power.usb_connected,
            'started':self.started,'configured':self.configured,'reset_pulses':self.reset_pulses,
            'registers':{hex(address):hex(value) for address,value in self.registers.items()},
            'writes':self.writes,
            'limits':['Bit31 ofFFF55104 is explicit cable presence, retained independently of written configuration.',
                      'Other register reset values are unmeasured zero fixtures; stock accesses retain only validated commands.',
                      'No ready bits, bus exchanges, endpoint data, enumeration, transfer completion or IRQ are invented.',
                      'Halfword core controls and word PHY controls are distinct; other offsets/widths/values reject.',
                      'The supported initial core values assume a zero-reset register fixture; connected USB operations remain unsupported.']}
        if self._endpoint is not None:
            state['endpoint_transport'] = self._endpoint.snapshot()
            state['limits'].append('Selected endpoint contract adds opaque FIFO/DMA/IRQ transport; it supplies no firmware command replies.')
        return state
