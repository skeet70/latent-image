"""SPI2 endpoint2 transmit-only transport from stock output instructions.

Endpoint2 configuration/CS and generic 9E08EC output are instruction-derived.
The stock caller waits for transport completion without consuming RX. This
model records shifted 16-bit words after a bounded virtual delay, without
modeling external register effects or inventing receive-data values.
"""
from .hardware import UnsupportedDevice


class SPI2Serial:
    BASE = 0xfff84080
    _transport = (0xe0, 4, 0)
    _selection = (13, 9, 0)
    _endpoint_id = 2
    _transfer_ticks = 4

    def __init__(self, board, *, endpoint=None, base=None, transport=None,
                 selection=None, endpoint_id=None, transfer_ticks=None):
        configured = any(value is not None for value in (base, transport, selection, endpoint_id, transfer_ticks))
        if configured:
            base = self.BASE if base is None else base
            transport = self._transport if transport is None else transport
            selection = self._selection if selection is None else selection
            endpoint_id = self._endpoint_id if endpoint_id is None else endpoint_id
            transfer_ticks = self._transfer_ticks if transfer_ticks is None else transfer_ticks
            if (type(base) is not int or not 0 <= base <= 0xffffffe0 or base & 0x1f
                    or type(transport) is not tuple or len(transport) != 3
                    or any(type(v) is not int or not 0 <= v <= 0xffffffff for v in transport)
                    or transport[0] & 1 or transport[2] != 0
                    or type(selection) is not tuple or len(selection) != 3
                    or any(type(v) is not int for v in selection)
                    or not (0 <= selection[0] < 18 and 0 <= selection[1] < 16 and selection[2] in (0, 1))
                    or type(endpoint_id) is not int or not 0 <= endpoint_id <= 255
                    or type(transfer_ticks) is not int or not 1 <= transfer_ticks < 1 << 64):
                raise ValueError('Unsupported SPI2 transmit-only contract')
            self.BASE, self._transport, self._selection = base, transport, selection
            self._endpoint_id, self._transfer_ticks = endpoint_id, transfer_ticks
        self.board = board
        self.endpoint = endpoint
        self.registers = {}
        self.busy = False
        self.generation = self.started = self.completed = 0
        self.words = []
        self.board.record('SPI2 idle controller fixture',
                          provenance='zero configuration and empty unused receive FIFO; no physical capture',
                          limits=('Only declared transmit-only transport; external device effects unresolved' if configured
                                  else 'Only endpoint2 transmit-only transport; external device effects unresolved'))

    def contains(self, address, size):
        return size == 4 and not address & 3 and address-self.BASE in range(0, 0x20, 4)

    def read(self, core, address, size):
        if not self.contains(address, size):
            raise UnsupportedDevice(f'Unknown SPI2 read {address:#x}/{size}')
        offset = address-self.BASE
        if offset == 0x18:
            raise UnsupportedDevice('SPI2 receive data is unresolved; only transmit-only callers are modeled')
        value = self.registers.get(address, 0)
        if offset == 0:
            value = (value & ~1) | int(self.busy)
        elif offset in (0x10, 0x14):
            # 9E08EC's generic status reader reads these fields but its
            # caller only consumes status bit9, not FIFO levels/data.
            # Retain explicitly empty unused queues, never a reply value.
            value &= ~0x1f
        return value

    def write(self, core, address, size, value):
        if not self.contains(address, size):
            raise UnsupportedDevice(f'Unknown SPI2 write {address:#x}/{size}')
        value &= 0xffffffff
        offset = address-self.BASE
        if offset == 0x18:
            raise UnsupportedDevice('SPI2 receive-data port is read-only and unresolved')
        if offset == 0x1c:
            self._start(core, value)
            return
        if offset == 0xc:
            previous = self.registers.get(address, 0)
            self.registers[address] = (value & 0xffff0000) | (previous & 0xffff & ~value)
        else:
            self.registers[address] = value
        if offset == 8 and value & 0x10000000:
            self.generation += 1
            self.busy = False
            self.registers[self.BASE+0xc] = self.registers.get(self.BASE+0xc, 0) & ~0x300
            self.board.record('SPI2 FIFO reset', cpu=core,
                              provenance='stock 917B4C pulse; cancels pending modeled shift')

    def _start(self, core, word):
        if self.busy:
            raise UnsupportedDevice('SPI2 transmit while shift is busy')
        config = tuple(self.registers.get(self.BASE+offset, 0) for offset in (0, 4, 8))
        if (config != self._transport or self.registers.get(self.BASE+0xc, 0) & 0xffff0000
                or not 0 <= word <= 0xffff):
            raise UnsupportedDevice(f'Unsupported SPI2 endpoint2 transport {config!r}, TX={word:#x}')
        pins = next((d for d in getattr(self.board, 'devices', ()) if hasattr(d, 'gpio_output_pin')), None)
        port, pin, level = self._selection
        if pins is None or pins.gpio_output_pin(port, pin) != level:
            raise UnsupportedDevice('SPI2 endpoint requires its declared actual GPIO chip select')
        self.registers[self.BASE+0x1c] = word
        self.generation += 1
        generation = self.generation
        self.started += 1
        self.busy = True
        self.board.record('SPI2 word shift start', cpu=core, endpoint=self._endpoint_id, word=hex(word),
                          select_port=port, select_pin=pin, select_level=level, shift_ticks=self._transfer_ticks,
                          provenance=('explicit unmeasured transport contract' if '_transport' in self.__dict__
                                      else 'stock AEA6E8 endpoint2 and 9E08EC transmit-only contract'))

        def complete():
            if generation != self.generation:
                return
            self.busy = False
            self.completed += 1
            self.registers[self.BASE+0xc] = self.registers.get(self.BASE+0xc, 0) | 0x300
            self.words.append({'cpu': core, 'word': hex(word)})
            if len(self.words) > 512:
                del self.words[:-512]
            self.board.record('SPI2 word shift complete', cpu=core, endpoint=self._endpoint_id, word=hex(word),
                              limits='Transmit recorded; no RX value, device register effects or physical timing modeled')
            if self.endpoint is not None:
                try:
                    self.endpoint.transmit(core, word)
                except UnsupportedDevice as error:
                    self.board.failure = dict(message=str(error), cpu=core,
                        peripheral=f'SPI2 endpoint{self._endpoint_id}', completed_word=hex(word))
                    self.board.record('SPI2 endpoint rejected completed word', **self.board.failure)
        self.board.schedule(self._transfer_ticks, complete)

    def snapshot(self):
        state = {'name': 'SPI2Serial', 'base': hex(self.BASE), 'busy': self.busy,
                'started': self.started, 'completed': self.completed,
                'retained_words': list(self.words),
                'registers': {hex(k): hex(v) for k, v in sorted(self.registers.items())},
                'limits': ['Endpoint2 E0/4, GPIO13/9 low; sixteen-bit transmit-only words.',
                           'Four virtual ticks and initial empty unused receive FIFO are explicit model choices.',
                           'Receive-data requests, other endpoints/configurations, IRQ/DMA and unknown widths stop.',
                           'Transport completion records bytes; attached endpoint effects are separately declared.',
                           'No receive values, actuators or physical timing are supplied by transport.']}
        if '_transport' in self.__dict__:
            state['configuration'] = {'transport': self._transport, 'selection': self._selection,
                                      'endpoint_id': self._endpoint_id, 'transfer_ticks': self._transfer_ticks}
            state['limits'][:2] = ['Explicit sixteen-bit transmit-only transport/configuration and GPIO contract.',
                                  'Latency and initial empty unused receive FIFO are unmeasured model choices.']
        return state
