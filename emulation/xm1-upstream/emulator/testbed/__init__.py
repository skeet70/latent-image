"""AF testbed layer over the native emulator; see README.md."""
from .display import BoundaryChanged, DisplayRecorder, NoFrame, Screenshot, display_changed, display_matches
from .events import EventBus, bus_for, emit
from .memory import watch
from .patch import install_ram_patch
from .predicates import (device_attr, event, lower_state, mem_changed, mem_equals, mem_pred,
                         rec_state, ui_mode)
from .testbed import RunResult, Testbed

__all__ = ['Testbed', 'RunResult', 'EventBus', 'bus_for', 'emit', 'watch', 'install_ram_patch',
           'mem_equals', 'mem_changed', 'mem_pred', 'device_attr', 'rec_state', 'lower_state',
           'event', 'ui_mode', 'DisplayRecorder', 'Screenshot', 'NoFrame', 'BoundaryChanged',
           'display_changed', 'display_matches']
