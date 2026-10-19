"""Interrupt-controller trace retention over long runs."""
import unittest

from emulator.interrupts import KernelInterrupts


class InterruptTraceTests(unittest.TestCase):
    def test_trace_is_bounded_and_recent_records_stay_addressable(self):
        controller = KernelInterrupts()
        for index in range(50_000):
            controller.raise_irq(index & 1, index % 128)
        self.assertLessEqual(len(controller.trace), KernelInterrupts.TRACE_LIMIT)

        start = controller.trace_position
        for line in (3, 5, 7):
            controller.raise_irq(1, line)
        recent = controller.trace_since(start)
        self.assertEqual([(item['name'], item['cpu_id'], item['line']) for item in recent],
                         [('Peripheral IRQ request', 1, line) for line in (3, 5, 7)])

    def test_records_since_a_position_survive_a_trim(self):
        controller = KernelInterrupts()
        start = controller.trace_position
        for index in range(KernelInterrupts.TRACE_LIMIT + 1):
            controller.raise_irq(0, index % 128)
        recent = controller.trace_since(start)
        self.assertEqual(recent[-1]['line'], KernelInterrupts.TRACE_LIMIT % 128)
        self.assertEqual(len(recent), len(controller.trace))


if __name__ == '__main__':
    unittest.main()
