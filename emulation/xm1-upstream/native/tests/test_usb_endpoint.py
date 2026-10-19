"""X-M1 endpoint transport selection for an ordinary post-boot attach."""
import gc
import unittest
from types import SimpleNamespace

from tools.continue_rec_scanout import _FF80EndpointBridge

from emulator.boards.profile import resolve
from emulator.boot import make_board
from emulator.power_devices import PowerDevices
from emulator.usb_devices import USBControl


class USBEndpointSelectionTests(unittest.TestCase):
    def setUp(self):
        profile = resolve('xm1-150', profile_file='firmware/x-m1/1.50/emulator-rec.json')
        self.board = make_board(entry='loader', quantum=8, storage_profile='erased',
                                firmware_profile=profile)
        self.power = next(device for device in self.board.devices if type(device) is PowerDevices)
        self.usb = next(device for device in self.board.devices if type(device) is USBControl)

    def tearDown(self):
        gc.collect()

    def test_endpoint_transport_is_available_before_post_boot_attach(self):
        self.assertFalse(self.power.usb_connected)
        self.assertTrue(self.usb.contains(0xfff5402c, 2))
        self.assertTrue(self.usb.contains(0xfff54034, 2))
        self.assertTrue(self.usb.contains(0xfff55824, 4))
        self.assertIn('endpoint_transport', self.usb.snapshot())
        self.power.set_usb_connected(True)
        self.assertTrue(self.power.usb_connected)
        self.assertTrue(self.usb.contains(0xfff5402c, 2))

    def test_ram_push_bulk_out_delivers_all_packets_before_dma_completion(self):
        class Endpoint:
            contract = SimpleNamespace(packet_bytes=64)

            def __init__(self):
                self.rx = {1: b''}
                self.rx_positions = {1: 0}
                self.dma_active = {'direction': 'out', 'exact': 164}
                self.dma = [{0x004: 0}]
                self.packets = []

            def host_out(self, endpoint, packet):
                self.assert_endpoint(endpoint)
                self.rx[1] = packet
                self.rx_positions[1] = len(packet)
                self.packets.append(packet)
                if sum(map(len, self.packets)) == 164:
                    self.dma_active = None

            @staticmethod
            def assert_endpoint(endpoint):
                if endpoint != 2:
                    raise AssertionError(endpoint)

        endpoint = Endpoint()
        bridge = object.__new__(_FF80EndpointBridge)
        bridge.endpoint = endpoint
        bridge.run_until = lambda label, predicate, budget: self.assertTrue(predicate(), label)
        bridge.run_for = lambda count: None
        payload = bytes(range(164))

        bridge.bulk_out(2, payload)

        self.assertEqual([64, 64, 36], list(map(len, endpoint.packets)))
        self.assertEqual(payload, b''.join(endpoint.packets))

    def test_ram_push_bulk_in_retries_consumed_tokens_until_guest_reply(self):
        class Endpoint:
            contract = SimpleNamespace(in_endpoints=((0x81, 1),), packet_bytes=64)

            def __init__(self):
                self.regs = {0x030: 0}
                self.tx = {1: []}
                self.tokens = 0
                self.dma = [{0x004: 0}]

            def host_in_token(self, endpoint):
                self.assert_endpoint(endpoint)
                self.tokens += 1
                self.regs[0x030] = 4
                if self.tokens == 3:
                    self.tx[1].append(b'reply')

            def host_in(self, endpoint, count):
                self.assert_endpoint(endpoint)
                return self.tx[1].pop(0)[:count]

            @staticmethod
            def assert_endpoint(endpoint):
                if endpoint != 0x81:
                    raise AssertionError(endpoint)

        endpoint = Endpoint()
        bridge = object.__new__(_FF80EndpointBridge)
        bridge.endpoint = endpoint
        def run_until(label, predicate, budget):
            for _ in range(4):
                if predicate():
                    return
                endpoint.regs[0x030] = 0
            self.fail(label)
        bridge.run_until = run_until
        bridge.run_for = lambda count: None

        reply = bridge.take_in(0x81, 5)

        self.assertEqual(b'reply', reply)
        self.assertEqual(3, endpoint.tokens)


    def test_ram_push_bulk_in_reports_guest_short_packet(self):
        class Endpoint:
            contract = SimpleNamespace(in_endpoints=((0x81, 1),), packet_bytes=64)
            regs = {0x030: 0}

            def __init__(self):
                self.tx = {1: [b'x' * 28]}
                self.dma = [{0x004: 0}]

            def host_in(self, endpoint, count):
                if endpoint != 0x81:
                    raise AssertionError(endpoint)
                return self.tx[1].pop(0)[:count]

        bridge = object.__new__(_FF80EndpointBridge)
        bridge.endpoint = Endpoint()
        bridge.run_until = lambda label, predicate, budget: self.assertTrue(predicate(), label)
        bridge.run_for = lambda count: None

        reply = bridge.take_in(0x81, 36)

        self.assertEqual(b'x' * 28, reply)




if __name__ == '__main__':
    unittest.main()
