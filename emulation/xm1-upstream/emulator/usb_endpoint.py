"""Opt-in EXR II USB FIFO/DMA transport; all payload bytes belong to the guest. No service commands, firmware pointers or descriptors are interpreted here. The host supplies setup/OUT bytes and consumes bytes written to TX FIFO/DMA. Core widths and event roles are bound to the X-M1 1.50 controller closure at 0x00d7af90..0x00d7dfd4; IRQ84/85 descriptors are checked by the profile. Timings and reset values remain synthetic inputs."""
from dataclasses import dataclass
from .hardware import UnsupportedDevice


@dataclass(frozen=True)
class USBEndpointContract:
    core: int = 0xfff54000
    phy: int = 0xfff55000
    irq: int = 84
    dma_irq: int = 85
    transfer_ticks: int = 4
    packet_bytes: int = 64
    dma_limit: int = 1 << 20
    out_endpoints: tuple = ((0, 0), (1, 1))
    in_endpoints: tuple = ((0x80, 0), (0x82, 1), (0x83, 2))
    control_packet_bytes: int = 64
    interrupt_packet_bytes: int = 32

    def __post_init__(self):
        if (self.core != 0xfff54000 or self.phy != 0xfff55000
                or type(self.irq) is not int or not 0 <= self.irq < 256
                or type(self.dma_irq) is not int or not 0 <= self.dma_irq < 256
                or self.irq == self.dma_irq
                or type(self.transfer_ticks) is not int or not 1 <= self.transfer_ticks < 1 << 32
                or self.packet_bytes not in (64, 512)
                or type(self.dma_limit) is not int or not 64 <= self.dma_limit <= 1 << 20):
            raise ValueError('Unadmitted USB endpoint contract')
        if self.control_packet_bytes != 64 or self.interrupt_packet_bytes != 32:
            raise ValueError('Unadmitted USB control/interrupt packet capacities')
        for rows, direction, channels in ((self.out_endpoints, 0, (0, 1)),
                                          (self.in_endpoints, 0x80, (0, 1, 2))):
            if (type(rows) is not tuple or not rows or
                    any(type(row) is not tuple or len(row) != 2 or
                        type(row[0]) is not int or not 0 <= row[0] <= 0x8f or
                        row[0] & 0x80 != direction or type(row[1]) is not int or
                        row[1] not in channels for row in rows) or
                    len({row[0] for row in rows}) != len(rows) or
                    len({row[1] for row in rows}) != len(rows)):
                raise ValueError('Unadmitted USB endpoint/port wiring')


class USBEndpointEngine:
    # X-M1 1.50 stock halfword and word accesses in 00d7af90..00d7dfd4.
    # CORE+0x194 has distinct halfword-read and word-write roles.
    HALF = frozenset((0x004, 0x008, 0x02c, 0x030, 0x034, 0x040, 0x044, 0x048,
        0x054, 0x058, 0x05c, 0x068, 0x06c, 0x070, 0x080, 0x088,
        0x090, 0x094, 0x098, 0x09c, 0x0ac, 0x184, 0x194, 0x1ec, 0x1f0, 0x1f4, 0x1f8, 0x218, 0x21c))
    WORD = frozenset((0x180, 0x188, 0x18c, 0x190, 0x194, 0x19c,
        0x1108, 0x110c, 0x1114, 0x111c, 0x1120, 0x1124, 0x1128, 0x112c))
    DMA = frozenset((0x004, 0x00c, 0x010, 0x020, 0x024, 0x028, 0x044))

    def __init__(self, board, contract):
        if not isinstance(contract, USBEndpointContract):
            raise TypeError('USB endpoint contract must be explicitly resolved')
        self.board, self.contract = board, contract
        self.regs = {offset: 0 for offset in self.HALF | self.WORD}
        self.regs[0x09c] = 0x4000 if contract.packet_bytes == 512 else 0
        self.dma = [{offset: 0 for offset in self.DMA} for _ in range(2)]
        self.rx = {0: b'', 1: b''}
        self.rx_positions = {0: 0, 1: 0}
        self.setup = b''
        self.setup_position = 0
        self.tx_staging = {port: bytearray() for port in range(3)}
        self.tx = {port: [] for port in range(3)}
        self.tx_partial = 4
        self.irq_asserted = False
        self.dma_generation = [0, 0]
        self.dma_active = None
        self.dma_exact_fresh = {'in': False, 'out': False}
        self.transfers = []
        board.record('USB endpoint transport contract', packet_bytes=contract.packet_bytes,
                     transfer_ticks=contract.transfer_ticks,
                     reset='synthetic zero registers, inactive FIFOs/DMA',
                     limits='opaque host/guest bytes; no command responder or guest context inference')

    def contains(self, address, size):
        offset = address - self.contract.core
        if (size == 2 and offset in self.HALF) or (size == 4 and offset in self.WORD):
            return True
        relative = address - (self.contract.phy + 0x800)
        return size == 4 and 0 <= relative < 0x100 and relative % 0x80 in self.DMA

    def mmio_ranges(self):
        return tuple((self.contract.core + offset, self.contract.core + offset + 1)
                     for offset in self.HALF | self.WORD) + tuple(
                     (self.contract.phy + 0x800 + channel * 0x80 + offset,
                      self.contract.phy + 0x801 + channel * 0x80 + offset)
                     for channel in range(2) for offset in self.DMA)

    def _route(self, line):
        # Source registration, rather than its temporary enabled mask, owns
        # completion. No callback/task/global pointer is consulted.
        controller = getattr(self.board, 'interrupts', None)
        if controller is None:
            raise UnsupportedDevice('USB physical IRQ requires a modeled interrupt controller')
        shift = (line % 16) * 2
        targets = [cpu for cpu, bank in enumerate(controller.banks)
                   if (bank.config.get(0x400 + line * 4, 0) and
                       (bank.source_config & 1 << line or
                        any((bank.config.get(base + line // 16 * 4, 0) >> shift) & 3
                            for base in (0x140, 0x180))))]
        if len(targets) != 1:
            raise UnsupportedDevice(f'USB physical IRQ{line} needs one registered source owner')
        return targets[0]

    def _binding(self, line, owner):
        group, shift = line // 32, (line % 16) * 2
        modes = (0x140 + line // 16 * 4, 0x180 + line // 16 * 4)
        vector, priority = 0x400 + line * 4, 0x800 + line * 4
        fields = (0x60 + group * 4, *modes, vector, priority)
        bank = self.board.interrupts.banks[owner]
        if (any(offset not in bank.config for offset in fields) or
                not 0 < bank.config[vector] <= 0xffffffff or bank.config[vector] & 3 or
                not 0 <= bank.config[priority] <= 15):
            raise UnsupportedDevice('USB IRQ owner lacks a complete vector/priority/mode binding')
        return (((bank.config[fields[0]] >> (line % 32)) & 1),
                ((bank.source_config >> line) & 1),
                *((bank.config[offset] >> shift) & 3 for offset in modes),
                bank.config[vector], bank.config[priority])

    def _pending_irq(self):
        active = any(self.regs[status] & ~self.regs[mask] & 0xffff
                     for status, mask in ((0x02c, 0x040), (0x030, 0x044), (0x034, 0x048)))
        if not active:
            self.irq_asserted = False
        elif not self.irq_asserted:
            owner = self._route(self.contract.irq)
            if self._binding(self.contract.irq, owner)[0]:
                raise UnsupportedDevice('USB endpoint source requires an IRQ route, not FIQ')
            self.board.queue_irq(owner, self.contract.irq)
            self.irq_asserted = True

    def _event(self, offset, bits):
        self.regs[offset] |= bits
        self._pending_irq()

    def host_reset(self):
        if self.dma_active is not None:
            raise UnsupportedDevice('Host reset during a modeled DMA transfer is not admitted')
        self.rx = {0: b'', 1: b''}
        self.rx_positions = {0: 0, 1: 0}
        self.setup, self.setup_position = b'', 0
        for port in self.tx_staging:
            self.tx_staging[port].clear()
            self.tx[port].clear()
        self._event(0x034, 0x1000)
        self.board.record('External USB bus reset', basis='explicit synthetic host reset')

    def host_setup(self, packet):
        if type(packet) is not bytes or len(packet) != 8 or self.setup_position < len(self.setup):
            raise ValueError('USB setup requires eight bytes and an empty setup FIFO')
        self.setup, self.setup_position = packet, 0
        self._event(0x02c, 0x2000)
        self.board.record('External USB setup packet', packet_hex=packet.hex())

    def host_out(self, endpoint, packet):
        channel = dict(self.contract.out_endpoints).get(endpoint)
        capacity = self.contract.control_packet_bytes if channel == 0 else self.contract.packet_bytes
        if channel is None or type(packet) is not bytes or not (0 if channel == 0 else 1) <= len(packet) <= capacity:
            raise ValueError('Unadmitted USB OUT endpoint/packet extent')
        if self.rx_positions[channel] < len(self.rx[channel]):
            raise BlockingIOError('USB OUT FIFO is occupied')
        self.rx[channel], self.rx_positions[channel] = packet, 0
        self.regs[0x184 + channel * 0x10] = len(packet)
        if channel == 1 and self.dma_active is not None and self.dma_active['direction'] == 'out':
            self._receive_dma()
        elif channel == 0 and not packet and self.setup and self.setup[0] & 0x80:
            self._event(0x034, 2)  # Control-read status OUT completion.
        else:
            self._event(0x02c, 1 if channel == 0 else 4)
        self.board.record('External USB OUT packet', endpoint=endpoint, packet_hex=packet.hex())

    def host_in(self, endpoint, size):
        port = dict(self.contract.in_endpoints).get(endpoint)
        if port is None or type(size) is not int or size <= 0:
            raise ValueError('Unadmitted USB IN endpoint/extent')
        if not self.tx[port]:
            raise BlockingIOError('USB IN endpoint has no guest-produced packet')
        packet = self.tx[port].pop(0)
        if len(packet) > size:
            self.tx[port].insert(0, packet[size:])
            packet = packet[:size]
        if not self.tx[port]:
            if port == 0 and not packet and self.setup and not self.setup[0] & 0x80:
                self._event(0x034, 2)  # Control-write status IN completion.
            else:
                self._event(0x02c, (2, 8, 0x10)[port])
        self.board.record('External USB IN packet', endpoint=endpoint, packet_hex=packet.hex())
        return packet

    def host_in_token(self, endpoint):
        """Request bulk IN data; stock class callback supplies its own bytes."""
        port = dict(self.contract.in_endpoints).get(endpoint)
        if port != 1 or self.tx[port]:
            raise ValueError('Bulk IN token requires a selected empty bulk FIFO')
        # 10cf2b8 tests middle status bit2 before calling the actual class
        # buffer provider and TX helper. Low bit3 is later TX completion.
        self._event(0x030, 4)
        self.board.record('External USB IN token', endpoint=endpoint,
                          basis='host request for bulk IN; no guest buffer or service command inference')

    def read(self, core, address, size):
        if not self.contains(address, size):
            raise UnsupportedDevice(f'Unknown USB endpoint read {address:#x}/{size}')
        relative = address - (self.contract.phy + 0x800)
        if 0 <= relative < 0x100:
            return self.dma[relative // 0x80][relative % 0x80]
        offset = address - self.contract.core
        if size == 4 and offset == 0x188:
            if self.setup_position >= len(self.setup):
                raise UnsupportedDevice('Read from empty USB setup FIFO')
            raw = self.setup[self.setup_position:self.setup_position + 4]
            self.setup_position += 4
            return int.from_bytes(raw, 'little')
        if size == 4 and offset in (0x180, 0x190):
            channel = (offset - 0x180) // 0x10
            position = self.rx_positions[channel]
            if position >= len(self.rx[channel]):
                raise UnsupportedDevice('Read from empty USB OUT FIFO')
            raw = self.rx[channel][position:position + 4]
            self.rx_positions[channel] += len(raw)
            self.regs[0x184 + channel * 0x10] = len(self.rx[channel]) - self.rx_positions[channel]
            if self.rx_positions[channel] == len(self.rx[channel]):
                self.regs[0x02c] &= ~(1 if channel == 0 else 4)
                self._pending_irq()  # FIFO-not-empty source deasserts on drain.
            return int.from_bytes(raw, 'little')
        if offset in (0x18c, 0x19c) or size == 4 and offset == 0x194:
            raise UnsupportedDevice('USB TX FIFO is write-only')
        return self.regs[offset]

    def write(self, core, address, size, value):
        if not self.contains(address, size) or type(value) is not int or not 0 <= value <= 0xffffffff:
            raise UnsupportedDevice(f'Unknown USB endpoint write {address:#x}/{size}')
        value &= (1 << (size * 8)) - 1
        relative = address - (self.contract.phy + 0x800)
        if 0 <= relative < 0x100:
            channel, offset = divmod(relative, 0x80)
            registers = self.dma[channel]
            if offset in (0x00c, 0x044):
                if value != 1:
                    raise UnsupportedDevice('USB DMA reset/ack requires bit0')
                registers[0x044] = 0
            elif offset == 0x004:
                if value & ~0x10001:
                    raise UnsupportedDevice('Unadmitted USB DMA control bits')
                if value & 0x10000:
                    self.dma_generation[channel] += 1
                    if self.dma_active is not None and self.dma_active['channel'] == channel:
                        self.dma_active = None
                    registers[0x010] = 0
                registers[offset] = value & ~0x10000
            elif offset == 0x010:
                raise UnsupportedDevice('USB DMA status is read-only')
            elif offset == 0x024 and value & 0xc0000003:
                raise UnsupportedDevice('USB DMA address must be an aligned physical DRAM address')
            elif offset == 0x028 and (value & 0xf0000003 or value > self.contract.dma_limit):
                raise UnsupportedDevice('USB DMA count exceeds the bounded aligned extent')
            else:
                registers[offset] = value
            return
        offset = address - self.contract.core
        if offset in (0x02c, 0x030, 0x034, 0x09c, 0x184, 0x1128) or size == 2 and offset == 0x194:
            raise UnsupportedDevice('USB event/count/speed input is read-only')
        if size == 4 and offset in (0x18c, 0x194, 0x19c):
            port = (offset - 0x18c) // 8
            payload = value.to_bytes(4, 'little')[:self.tx_partial]
            capacity = (self.contract.control_packet_bytes if port == 0 else
                        self.contract.interrupt_packet_bytes if port == 2 else self.contract.packet_bytes)
            if len(self.tx_staging[port]) + len(payload) > capacity:
                raise UnsupportedDevice('USB FIFO packet exceeds selected packet size')
            self.tx_staging[port].extend(payload)
        elif size == 4 and offset in (0x180, 0x188, 0x190):
            raise UnsupportedDevice('USB receive/setup FIFO is read-only')
        elif offset == 0x111c:
            if value in (0x80000, 0xc0000, 0xe0000):
                self.tx_partial = {0x80000: 3, 0xc0000: 2, 0xe0000: 1}[value]
            elif value in (0x10, 0x11, 0x12, 0x13):
                self.tx_partial = 4
            else:
                raise UnsupportedDevice('Unadmitted USB FIFO partial-word control')
            self.regs[offset] = value
        elif offset == 0x088:
            if value not in (1, 2, 4):
                raise UnsupportedDevice('USB FIFO commit requires exactly one admitted port')
            port = value.bit_length() - 1
            self.tx[port].append(bytes(self.tx_staging[port]))
            self.tx_staging[port].clear()
        elif offset in (0x054, 0x058, 0x05c):
            status = {0x054: 0x02c, 0x058: 0x030, 0x05c: 0x034}[offset]
            self.regs[status] &= value  # stock active-low event acknowledgement
            self.regs[offset] = value
            self._pending_irq()
        elif offset == 0x080:
            if value & ~0xfff:
                raise UnsupportedDevice('Unadmitted USB FIFO reset bits')
            self.regs[offset] = value
        else:
            self.regs[offset] = value
            if offset in (0x040, 0x044, 0x048):
                self._pending_irq()
            if offset in (0x1120,0x1124):
                self.dma_exact_fresh['in' if offset==0x1124 else 'out']=True
            if offset in (0x110c,0x1120,0x1124):
                self._try_dma()

    def _try_dma(self):
        if self.dma_active is not None or not self.regs[0x110c] & 1:
            return
        enabled = [(channel, r) for channel, r in enumerate(self.dma) if r[0x004] & 1]
        if len(enabled) != 1:
            raise UnsupportedDevice('USB DMA requires exactly one enabled transfer channel')
        channel, r = enabled[0]
        # Exact lengths can be written before or after the arm register.
        # A consumed length from an earlier transfer must not start a new job.
        directions=[name for name,bit in (('in',2),('out',1))
                    if self.regs[0x070]&bit and self.dma_exact_fresh[name]]
        if not directions:return
        if len(directions)!=1:raise UnsupportedDevice('USB DMA has ambiguous fresh IN/OUT lengths')
        direction=directions[0]
        count, address = r[0x028], r[0x024]
        exact = self.regs[0x1124 if direction == 'in' else 0x1120]
        if not 0 < exact <= count <= self.contract.dma_limit or address + count > len(self.board.ram):
            raise UnsupportedDevice('USB DMA address/count/endpoint extent does not agree')
        if r[0x020] & 0x80703000 != 0x80401000:
            raise UnsupportedDevice('USB DMA requires stock unit4, burst1 transfer grammar')
        self.dma_exact_fresh[direction]=False
        self.dma_active = dict(channel=channel, direction=direction, address=address,
                               count=count, exact=exact, received=bytearray())
        r[0x010] = 1
        if direction == 'in':
            payload = bytes(self.board.read_memory(address, exact))
            self._complete_dma(payload)
        else:
            self._receive_dma()

    def _receive_dma(self):
        active = self.dma_active
        if active is None or active['direction'] != 'out':
            return
        available = self.rx[1][self.rx_positions[1]:]
        if not available:
            return
        remaining = active['exact'] - len(active['received'])
        if len(available) > remaining:
            raise UnsupportedDevice('USB OUT packet exceeds programmed DMA transfer')
        active['received'].extend(available)
        self.rx_positions[1] = len(self.rx[1])
        self.regs[0x194] = 0
        self.regs[0x02c] &= ~4
        self._pending_irq()
        if len(active['received']) == active['exact']:
            self._complete_dma(bytes(active['received']))

    def _complete_dma(self, payload):
        active = self.dma_active
        channel = active['channel']
        generation = self.dma_generation[channel]
        irq_owner = self._route(self.contract.dma_irq)
        binding = self._binding(self.contract.dma_irq, irq_owner)
        if binding[0]:
            raise UnsupportedDevice('USB DMA source requires an IRQ route, not FIQ')
        def finish():
            if self.dma_generation[channel] != generation:
                return
            if (self._route(self.contract.dma_irq) != irq_owner or
                    self._binding(self.contract.dma_irq, irq_owner) != binding):
                raise UnsupportedDevice('USB DMA source/vector/priority/mode binding changed during transfer')
            if active['direction'] == 'in':
                capacity = self.contract.packet_bytes
                self.tx[1].extend(payload[offset:offset + capacity]
                                  for offset in range(0, len(payload), capacity))
                self._event(0x034, 0x40)
            else:
                self.board.write_memory(active['address'], payload)
            self.dma[channel][0x010] = 0
            self.dma[channel][0x044] = 1
            self.dma_active = None
            self.transfers.append(dict(channel=channel, direction=active['direction'],
                address=hex(active['address']), count=active['count'], exact=active['exact'], payload_hex=payload.hex()))
            if self.dma[channel][0x020] & 0x10000000:
                self.board.queue_irq(irq_owner, self.contract.dma_irq)
        self.board.schedule(self.contract.transfer_ticks, finish)

    def snapshot(self):
        return dict(registers={hex(k):hex(v) for k,v in self.regs.items()},
                    dma=[dict(r) for r in self.dma], setup_hex=self.setup.hex(),
                    setup_position=self.setup_position, rx={k:v.hex() for k,v in self.rx.items()},
                    rx_positions=dict(self.rx_positions), tx={k:[v.hex() for v in values] for k,values in self.tx.items()},
                    transfers=list(self.transfers), transfer_ticks=self.contract.transfer_ticks,
                    packet_bytes=self.contract.packet_bytes,
                    binding='X-M1 1.50 00d7af90..00d7dfd4 core closure and checked IRQ84/85 descriptor triples',
                    limits='Synthetic register reset/latency/speed; opaque byte transport; no firmware command or descriptor emulation')
