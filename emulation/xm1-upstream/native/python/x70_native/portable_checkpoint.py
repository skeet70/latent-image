"""Versioned, plain native checkpoints reconstructed in fresh engines.

Only the stock factory topology and drained strict scheduler boundaries are
supported. Files contain typed model data and explicit architectural fields;
never Unicorn contexts, executable callbacks, pickle, or process pointers.
The same Python/models/native/Unicorn build is required. Physical-device
assumptions remain those of the selected profiles, not measured mechanics.
"""
from __future__ import annotations

from collections import deque
from collections.abc import Mapping
import ctypes
from dataclasses import fields, is_dataclass
import gzip
import hashlib
import inspect
import json
import math
from pathlib import Path
import shutil
import sys
from types import FunctionType, MappingProxyType, MethodType

from emulator import checkpoint as reference
from emulator.provenance import _class, _code, _loaded_module
from .board import RustBoard, NativeCore
from .architecture import export_cpu, import_cpu, import_tcm, validate_cpu_state, ArchitectureError
from .checkpoint import _require_native_modes_disabled
from .provenance import provenance


class PortableCheckpointError(reference.CheckpointError):
    """The explicit portable format cannot preserve or reconstruct this state."""


SCHEMA = "x70-native-portable-checkpoint-v1"
MAX_STATE_BYTES = 128 << 20
MAX_NODES = 1_000_000
PAGE_SIZE = reference.PAGE_SIZE
STOCK_DEVICE_NAMES = ("BootControls", "KernelInterrupts", "NandController", "FujiDecompressor",
    "LoaderMemoryDMA", "USBControl", "PowerDevices", "AudioSerialControl", "PulseOutputs",
    "LensEndpoint", "SPI2Serial", "ControlBus", "SecondaryControlBus", "FE80Control",
    "InputProcessingControl", "InputAuxiliaryControl", "ProducerControl", "ImageParameterControl",
    "ImageWorkControl", "FaceAnalysisControl", "HDMIStartup", "OutputControl",
    "GraphicsAccelerator", "DisplayScanout", "DisplayTiming", "DisplayFormatter")
FACTORY_METHODS = {"read_bytes": "read_memory", "write_bytes": "write_memory",
                   "schedule": "schedule", "record": "record", "raise_irq": "queue_irq"}
VALUE_FIELDS = {"emulator.loader_devices.FujiLZStream": ("history", "pending"),
                "emulator.panel_serial.PanelSerial": ("board", "words_received", "words", "_transport", "_selection", "_framing", "ENDPOINT"),
                "emulator.sd_card.SparseCard": ("sectors", "max_dirty_sectors", "data", "base_image")}
# Finite execution/reconstruction helpers used by the stock device family.
# Pin their own globals/private functions as well as imported function bodies.
# Diagnostic server and process-provenance timestamp modules are excluded.
IDENTITY_HELPER_MODULES = ("emulator.histogram_devices", "emulator.histogram_protocol",
    "emulator.boards.firmware_blobs",
    "emulator.nand_image",
    "emulator.producer_shutdown", "emulator.producer_preview", "emulator.producer_measurement",
    "emulator.afs_measurement_data", "emulator.measurement_scene", "emulator.lens_measurement_iris",
    "emulator.lens_af_motion", "emulator.lens_focus_motion", "emulator.producer_afs_focus",
    "emulator.afs_contrast_clock", "emulator.afs_focus_data", "emulator.afs_focus_scene",
    "emulator.afs_preview_return_clock",
    "emulator.afs_capture_clock", "emulator.afs_capture_start_clock", "emulator.afs_capture_stop_clock", "emulator.producer_capture_metstats",
    "emulator.producer_still_capture",
    "emulator.producer_stream", "emulator.image_stream",
    "emulator.afs_still_initialization_clock", "emulator.afs_still_qualifying_clock", "emulator.afs_still_readout_clock", "emulator.afs_still_tail_clock", "emulator.afs_still_image_clock", "emulator.afs_still_readout", "emulator.afs_still_raw", "emulator.afs_raw_source",
    "emulator.image_still_reset", "emulator.image_still_configuration", "emulator.image_still_prepare", "emulator.image_still_activation", "emulator.afs_still_image_delivery", "emulator.afs_still_image", "emulator.afs_still_numeric", "emulator.virtual_source", "emulator.graphics_clear_profile", "emulator.display",
    "emulator.recorded_statistics", "emulator.preview_statistics", "emulator.panel_serial", "emulator.checkpoint",
    "emulator.control_bus_contracts",
    "emulator.secondary_control_bus_contracts",
    "emulator.captured_mcu_profile", "emulator.storage_profile", "emulator.still_processing_prelude",
    "emulator.afs_still_cleanup_clock", "emulator.face_still_reconfiguration", "emulator.face_still_cancellation", "emulator.afs_startup_scope", "emulator.prefilter_initialization", "emulator.prefilter_active", "emulator.prefilter_scene", "emulator.prefilter_second", "emulator.input_still_return", "emulator.prefilter_third", "emulator.prefilter_empty", "emulator.still_f1c_idle", "emulator.producer_processing_reset", "emulator.image_processing_reset", "emulator.still_f1c_processing", "emulator.still_f1c_scene", "emulator.producer_processing_configuration", "emulator.image_processing_configuration", "emulator.still_f1c_gain", "emulator.processing_passive_configuration", "emulator.processing_control_configuration", "emulator.processing_pipeline_reset", "emulator.still_resize_successor", "emulator.still_resize_preview", "emulator.still_resize_thumbnail", "emulator.jpeg_successor", "emulator.image_processing_activation", "emulator.still_resize_jpeg_stream", "emulator.jpeg_streamed", "emulator.input_processing_activation", "emulator.still_resize_jpeg_main_stream", "emulator.processing_image_delivery", "emulator.processing_image_scene")


def _json(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def _name(cls):
    return cls.__module__ + "." + cls.__qualname__


def _profile_options(profile):
    from emulator.boards.profile import ROOT, ResolvedProfile, relative_path, thaw
    if type(profile) is not ResolvedProfile or profile.root.resolve() != ROOT.resolve():
        raise PortableCheckpointError("Selected profile must use the factory repository root")
    try:
        selected = profile.profile_file.resolve().relative_to(ROOT.resolve()).as_posix()
        if relative_path(ROOT, selected) != profile.profile_file.resolve():
            raise ValueError("Profile path differs")
    except ValueError as error:
        raise PortableCheckpointError("Selected profile path escapes the factory repository") from error
    resolved = dict(soc=thaw(profile.soc), board=thaw(profile.board),
                    firmware=thaw(profile.firmware), manifest=profile.manifest,
                    flash_sha256=_sha(profile.flash))
    return dict(firmware_id=profile.firmware_id, profile_file=selected,
                profile_sha256=profile.profile_sha256, part_sha256=thaw(profile.part_sha256),
                resolved_sha256=_sha(_json(resolved)))


def _resolve_factory_profile(options):
    from emulator.boards.profile import ROOT, relative_path, resolve
    from tools.firmware_registry import RegistryError
    if (type(options) is not dict or set(options) != {
            "firmware_id", "profile_file", "profile_sha256", "part_sha256", "resolved_sha256"}
            or type(options["firmware_id"]) is not str or not options["firmware_id"]
            or type(options["profile_file"]) is not str):
        raise PortableCheckpointError("Selected profile reconstruction schema differs")
    try:
        selected = relative_path(ROOT, options["profile_file"])
        if selected.relative_to(ROOT.resolve()).as_posix() != options["profile_file"]:
            raise ValueError("Profile path is not canonical")
        if _sha(selected.read_bytes()) != options["profile_sha256"]:
            raise PortableCheckpointError("Selected profile document SHA-256 differs")
        profile = resolve(options["firmware_id"], profile_file=options["profile_file"])
    except (RegistryError, ValueError, OSError) as error:
        raise PortableCheckpointError(f"Selected profile input is invalid or unavailable: {error}") from error
    if _profile_options(profile) != options:
        raise PortableCheckpointError("Selected profile inputs or resolved data differ")
    return profile


def _value_classes():
    # Trusted, finite model value types, not import names taken from a file.
    from emulator.interrupts import InterruptBank, PolledTimer, ApplicationTimer
    from emulator.display_timing import TimingBank
    from emulator.display_formatter import FormatterBank
    from emulator.loader_devices import FujiLZStream, PeripheralDmaState
    from emulator.panel_serial import PanelSerial
    from emulator.recorded_statistics import RecordedStatisticsFixture
    from emulator.sd_card import SparseCard
    from emulator.pulse_devices import PulseTimer
    from emulator.input_processing_devices import PhaseClockEdge
    from emulator.boot_devices import AcquisitionActuator
    from emulator.producer_stream import StatisticsFrame
    from emulator.control_bus_contracts import ControlBusRegisters, ControlBusReplies
    from emulator.boards.control_bus_parameters import EndpointDescriptor, FirmwareView
    return {_name(cls): cls for cls in (InterruptBank, PolledTimer, ApplicationTimer,
        TimingBank, FormatterBank, FujiLZStream, PeripheralDmaState, PulseTimer, PhaseClockEdge, AcquisitionActuator,
        ControlBusRegisters, ControlBusReplies, EndpointDescriptor, FirmwareView,
        PanelSerial, RecordedStatisticsFixture, SparseCard, StatisticsFrame)}


def _enum_classes():
    from emulator.audio_devices import AudioRegister
    from emulator.input_processing_devices import PhaseSource
    from emulator.pulse_devices import PulseClockProfile
    from emulator.boot_devices import AcquisitionActuatorProfile, AcquisitionDrive
    from emulator.image_stream import ImageStreamProfile
    from emulator.producer_stream import ProducerStreamProfile
    from emulator.scanout import RectangleProfile
    return {_name(cls): cls for cls in (AudioRegister, PhaseSource, PulseClockProfile,
                                      AcquisitionActuatorProfile, AcquisitionDrive, ImageStreamProfile,
                                      ProducerStreamProfile, RectangleProfile)}


def _validate_value(value):
    if _name(type(value)) == "emulator.producer_stream.StatisticsFrame":
        try:
            value.__post_init__()
        except ValueError as error:
            raise PortableCheckpointError("Statistics frame value contract differs") from error
    if _name(type(value)) == "emulator.boot_devices.AcquisitionActuator":
        from emulator.boot_devices import AcquisitionActuatorProfile, AcquisitionDrive
        if (type(value.profile) is not AcquisitionActuatorProfile
                or type(value.phase) is not AcquisitionDrive
                or type(value.settle_ticks) is not int or value.settle_ticks < 1
                or type(value.transitions) is not int or value.transitions < 0
                or (value.due_tick is not None
                    and (type(value.due_tick) is not int or value.due_tick < 0))):
            raise PortableCheckpointError("Acquisition actuator value contract differs")
    if _name(type(value)) == "emulator.panel_serial.PanelSerial":
        from emulator.panel_serial import PanelSerial
        try:
            if any(type(part) is not tuple for part in (value._transport, value._selection, value._framing)):
                raise ValueError("Panel serial contract fields require retained tuples")
            PanelSerial.parameters(value._transport, value._selection, value._framing)
            if type(value.ENDPOINT) is not int or not 0 <= value.ENDPOINT <= 255:
                raise ValueError("Invalid panel endpoint identifier")
        except ValueError as error:
            raise PortableCheckpointError("Panel serial value contract differs") from error
    if _name(type(value)) == "emulator.sd_card.SparseCard":
        from emulator.sd_card import SparseCard
        try:
            SparseCard(value.sectors, max_dirty_sectors=value.max_dirty_sectors)
        except ValueError as error:
            raise PortableCheckpointError("SparseCard capacity/bound differs") from error
        if (type(value.base_image) is not bytes or len(value.base_image) > 128 << 20
                or len(value.base_image) % 512 or len(value.base_image) > value.sectors * 512):
            raise PortableCheckpointError("SparseCard immutable base extent differs")
        if (type(value.data) is not dict or len(value.data) > value.max_dirty_sectors
                or any(type(sector) is not int or not 0 <= sector < value.sectors
                       or type(block) is not bytes or len(block) != 512 or block == value._base_sector(sector)
                       for sector, block in value.data.items())):
            raise PortableCheckpointError("SparseCard sector data differs")
    if _name(type(value)) in ("emulator.control_bus_contracts.ControlBusRegisters",
                             "emulator.control_bus_contracts.ControlBusReplies"):
        try:
            value.__post_init__()
        except ValueError as error:
            raise PortableCheckpointError("Control-bus value contract differs") from error


def _validate_acquisition_state(state, virtual_ticks):
    from emulator.boot_devices import AcquisitionActuator, AcquisitionDrive
    if "acquisition_actuator" not in state:
        raise PortableCheckpointError("Acquisition actuator state field is absent")
    actuator = state["acquisition_actuator"]
    if actuator is None:
        return
    if type(actuator) is not AcquisitionActuator:
        raise PortableCheckpointError("Acquisition actuator state type differs")
    _validate_value(actuator)
    if type(virtual_ticks) is not int or virtual_ticks < 0:
        raise PortableCheckpointError("Acquisition actuator board clock differs")
    levels, known = state.get("gpio_output_levels"), state.get("gpio_output_known")
    if (type(levels) is not dict or type(known) is not dict
            or any(type(words.get(1, 0)) is not int or not 0 <= words.get(1, 0) <= 0xffff
                   for words in (levels, known))):
        raise PortableCheckpointError("Acquisition actuator drive words differ")
    drive_known = known.get(1, 0) & 6
    if drive_known and 1 not in levels:
        raise PortableCheckpointError("Acquisition actuator drive levels are absent")
    if actuator.due_tick is not None and not drive_known:
        raise PortableCheckpointError("Acquisition actuator deadline lacks a drive command")
    if drive_known == 6:
        phase = {0: AcquisitionDrive.IDLE, 2: AcquisitionDrive.DRIVE,
                 6: AcquisitionDrive.COAST}.get(levels[1] & 6)
        if actuator.due_tick is None and phase is not actuator.phase:
            raise PortableCheckpointError("Acquisition actuator pending motion differs")


def _callback_identity(value):
    function = value.__func__ if isinstance(value, MethodType) else value
    return {"module": function.__module__, "qualname": function.__qualname__,
            "code_sha256": _sha(_json(_code(function.__code__)))}


def _identity(board):
    """Loaded model/code and binary identity, independent of process addresses."""
    from emulator import boot
    from emulator import preview_statistics  # Lazy import makes the curated helper available for pinning.
    from emulator import lens_measurement_iris  # Iris is explicitly armed only after its stock request.
    from . import architecture
    native = provenance()
    value_classes = _value_classes()
    modules = {type(device).__module__ for device in board.devices}
    modules.update(("emulator.board", "emulator.hardware", "emulator.boot"))
    if board._portable_factory_options.get("firmware_profile") is not None:
        modules.update(name for name in (
            "emulator.boards.profile", "emulator.boards.profile_layout", "emulator.boards.bindings",
            "emulator.boards.cpu_parameters", "emulator.boards.wiring", "emulator.boards.service_parameters",
            "emulator.boards.control_bus_parameters", "emulator.fe80_contracts", "tools.firmware_registry")
            if name in sys.modules)
    modules.update(name for name in IDENTITY_HELPER_MODULES
                   if name not in ("emulator.lens_af_motion", "emulator.lens_focus_motion",
                                   "emulator.producer_afs_focus", "emulator.afs_contrast_clock",
                                   "emulator.afs_focus_data", "emulator.afs_focus_scene",
                                   "emulator.afs_preview_return_clock", "emulator.afs_capture_clock", "emulator.afs_capture_start_clock", "emulator.afs_capture_stop_clock",
                                   "emulator.producer_capture_metstats", "emulator.producer_still_capture", "emulator.afs_still_initialization_clock", "emulator.afs_still_qualifying_clock", "emulator.afs_still_readout_clock", "emulator.afs_still_tail_clock", "emulator.afs_still_image_clock", "emulator.afs_still_readout",
                                   "emulator.afs_still_raw", "emulator.afs_raw_source", "emulator.image_still_reset", "emulator.image_still_configuration", "emulator.image_still_prepare", "emulator.image_still_activation", "emulator.afs_still_image_delivery", "emulator.afs_still_image", "emulator.afs_still_numeric", "emulator.still_processing_prelude", "emulator.afs_still_cleanup_clock", "emulator.face_still_reconfiguration", "emulator.face_still_cancellation", "emulator.afs_startup_scope", "emulator.prefilter_initialization", "emulator.prefilter_active", "emulator.prefilter_scene", "emulator.prefilter_second", "emulator.input_still_return", "emulator.prefilter_third", "emulator.prefilter_empty", "emulator.still_f1c_idle", "emulator.producer_processing_reset", "emulator.image_processing_reset", "emulator.still_f1c_processing", "emulator.still_f1c_scene", "emulator.producer_processing_configuration", "emulator.image_processing_configuration", "emulator.still_f1c_gain", "emulator.processing_passive_configuration", "emulator.processing_control_configuration", "emulator.processing_pipeline_reset", "emulator.still_resize_successor", "emulator.still_resize_preview", "emulator.still_resize_thumbnail", "emulator.jpeg_successor", "emulator.image_processing_activation", "emulator.still_resize_jpeg_stream", "emulator.jpeg_streamed", "emulator.input_processing_activation", "emulator.still_resize_jpeg_main_stream", "emulator.processing_image_delivery", "emulator.processing_image_scene"))
    # Older frozen endpoints have no AF motion API or helper source. New
    # endpoints pin the complete helper even before its explicit selection.
    if any(type(device).__module__ == "emulator.image_work_devices"
           and callable(getattr(device, "arm_still_image_reset", None)) for device in board.devices):
        from emulator import image_still_reset
        modules.add("emulator.image_still_reset")
        if any(callable(getattr(device,"arm_still_image_configuration",None)) for device in board.devices
               if type(device).__module__=="emulator.image_work_devices"):
            from emulator import image_still_configuration
            modules.add("emulator.image_still_configuration")
        if any(callable(getattr(device,"arm_still_image_prepare",None)) for device in board.devices
               if type(device).__module__=="emulator.image_work_devices"):
            from emulator import image_still_prepare
            modules.add("emulator.image_still_prepare")
        if any(callable(getattr(device,"arm_still_image_activation",None)) for device in board.devices
               if type(device).__module__=="emulator.image_work_devices"):
            from emulator import image_still_activation
            modules.add("emulator.image_still_activation")
        if any(callable(getattr(device,"arm_still_image_output",None)) for device in board.devices
               if type(device).__module__=="emulator.image_work_devices"):
            from emulator import afs_still_image_delivery,afs_still_image
            modules.update(("emulator.afs_still_image_delivery","emulator.afs_still_image"))
    if any(type(device).__module__ == "emulator.lens_endpoint"
           and callable(getattr(device, "enable_afs_motion", None)) for device in board.devices):
        from emulator import lens_af_motion
        modules.add("emulator.lens_af_motion")
        if callable(getattr(lens_af_motion, "af_motor_timer_selected", None)):
            from emulator import lens_focus_motion
            modules.add("emulator.lens_focus_motion")
    if any(type(device).__module__ == "emulator.producer_devices"
           and callable(getattr(device, "arm_afs_focus", None)) for device in board.devices):
        from emulator import producer_afs_focus
        modules.add("emulator.producer_afs_focus")
    if any(type(device).__module__ == "emulator.afs_frame_source"
           and callable(getattr(device, "arm_af_control", None)) for device in board.devices):
        from emulator import afs_contrast_clock
        modules.add("emulator.afs_contrast_clock")
    if any(type(device).__module__ == "emulator.afs_frame_source"
           and callable(getattr(device, "arm_preview_return", None)) for device in board.devices):
        from emulator import afs_preview_return_clock
        modules.add("emulator.afs_preview_return_clock")
    if any(type(device).__module__ == "emulator.afs_frame_source"
           and callable(getattr(device, "arm_capture_initialization", None)) for device in board.devices):
        from emulator import afs_capture_clock
        modules.add("emulator.afs_capture_clock")
    if any(type(device).__module__ == "emulator.afs_frame_source"
           and callable(getattr(device, "arm_capture_finite_stop", None)) for device in board.devices):
        from emulator import afs_capture_stop_clock
        modules.add("emulator.afs_capture_stop_clock")
    if any(type(device).__module__ == "emulator.afs_frame_source"
           and callable(getattr(device, "arm_capture_acquisition_start", None)) for device in board.devices):
        from emulator import afs_capture_start_clock
        modules.add("emulator.afs_capture_start_clock")
    if any(type(device).__module__ == "emulator.producer_devices"
           and callable(getattr(device, "arm_capture_metstats", None)) for device in board.devices):
        from emulator import producer_capture_metstats
        modules.add("emulator.producer_capture_metstats")
    if any(type(device).__module__ == "emulator.producer_devices"
           and callable(getattr(device, "arm_still_capture", None)) for device in board.devices):
        from emulator import producer_still_capture
        modules.add("emulator.producer_still_capture")
        if any(callable(getattr(device,"arm_still_statistics",None)) for device in board.devices
               if type(device).__module__=="emulator.producer_devices"):
            from emulator import afs_still_numeric
            modules.add("emulator.afs_still_numeric")
    if any(type(device).__module__ == "emulator.afs_frame_source"
           and callable(getattr(device, "arm_still_initialization", None)) for device in board.devices):
        from emulator import afs_still_initialization_clock
        modules.add("emulator.afs_still_initialization_clock")
        if any(callable(getattr(device, "arm_still_initialization_next", None)) for device in board.devices
               if type(device).__module__ == "emulator.afs_frame_source"):
            from emulator import afs_still_qualifying_clock
            modules.add("emulator.afs_still_qualifying_clock")
        if any(callable(getattr(device, "arm_still_readout_start", None)) for device in board.devices
               if type(device).__module__ == "emulator.afs_frame_source"):
            from emulator import afs_still_readout_clock
            modules.add("emulator.afs_still_readout_clock")
    if any(type(device).__module__ == "emulator.afs_frame_source"
           and callable(getattr(device,"arm_still_tail_boundary",None)) for device in board.devices):
        from emulator import afs_still_tail_clock
        modules.add("emulator.afs_still_tail_clock")
    if any(type(device).__module__ == "emulator.afs_frame_source"
           and callable(getattr(device,"arm_still_image_boundary",None)) for device in board.devices):
        from emulator import afs_still_image_clock
        modules.add("emulator.afs_still_image_clock")
    if any(type(device).__module__ == "emulator.afs_frame_source"
           and callable(getattr(device,"arm_still_cleanup_start",None)) for device in board.devices):
        from emulator import afs_still_cleanup_clock
        modules.add("emulator.afs_still_cleanup_clock")
    if any(type(device).__module__ == "emulator.face_analysis_devices"
           and callable(getattr(device,"arm_still_reconfiguration",None)) for device in board.devices):
        from emulator import face_still_reconfiguration
        modules.add("emulator.face_still_reconfiguration")
    if any(type(device).__module__ == "emulator.face_analysis_devices"
           and callable(getattr(device,"arm_still_cancellation",None)) for device in board.devices):
        from emulator import face_still_cancellation
        modules.add("emulator.face_still_cancellation")
    if any(type(device).__module__ == "emulator.still_processing_devices"
           and callable(getattr(device,"arm_prefilter_initialization",None)) for device in board.devices):
        from emulator import prefilter_initialization
        modules.add("emulator.prefilter_initialization")
    if any(type(device).__module__ == "emulator.still_processing_devices"
           and callable(getattr(device,"arm_prefilter_scene",None)) for device in board.devices):
        from emulator import prefilter_active, prefilter_scene
        modules.update(("emulator.prefilter_active", "emulator.prefilter_scene"))
    if any(type(device).__module__ == "emulator.still_processing_devices"
           and callable(getattr(device,"arm_prefilter_second_scene",None)) for device in board.devices):
        from emulator import prefilter_second
        modules.add("emulator.prefilter_second")
    if any(type(device).__module__ == "emulator.still_processing_devices"
           and callable(getattr(device,"arm_prefilter_third_scene",None)) for device in board.devices):
        from emulator import prefilter_third
        modules.add("emulator.prefilter_third")
    if any(type(device).__module__ == "emulator.still_processing_devices"
           and callable(getattr(device,"arm_prefilter_empty_scene",None)) for device in board.devices):
        from emulator import prefilter_empty
        modules.add("emulator.prefilter_empty")
    if any(type(device).__module__ == "emulator.still_processing_devices"
           and callable(getattr(device,"arm_f1c_idle",None)) for device in board.devices):
        from emulator import still_f1c_idle
        modules.add("emulator.still_f1c_idle")
    if any(type(device).__module__ == "emulator.still_processing_devices"
           and callable(getattr(device,"arm_f1c_processing",None)) for device in board.devices):
        from emulator import still_f1c_processing, still_f1c_scene
        modules.update(("emulator.still_f1c_processing", "emulator.still_f1c_scene"))
        if callable(getattr(still_f1c_scene,"prepare_gain",None)):
            from emulator import still_f1c_gain
            modules.add("emulator.still_f1c_gain")
    if any(type(device).__module__ == "emulator.producer_devices"
           and callable(getattr(device,"arm_processing_reset",None)) for device in board.devices):
        from emulator import producer_processing_reset
        modules.add("emulator.producer_processing_reset")
    if any(type(device).__module__ == "emulator.producer_devices"
           and callable(getattr(device,"arm_processing_configuration",None)) for device in board.devices):
        from emulator import producer_processing_configuration
        modules.add("emulator.producer_processing_configuration")
    if any(type(device).__module__ == "emulator.producer_devices"
           and callable(getattr(device,"arm_processing_passive_configuration",None)) for device in board.devices):
        from emulator import processing_passive_configuration
        modules.add("emulator.processing_passive_configuration")
    if any(type(device).__module__ == "emulator.producer_devices"
           and callable(getattr(device,"arm_processing_control_configuration",None)) for device in board.devices):
        from emulator import processing_control_configuration
        modules.add("emulator.processing_control_configuration")
    if any(type(device).__module__ == "emulator.producer_devices"
           and callable(getattr(device,"arm_processing_pipeline_reset",None)) for device in board.devices):
        from emulator import processing_pipeline_reset
        modules.add("emulator.processing_pipeline_reset")
    if any(type(device).__module__ == "emulator.image_work_devices"
           and callable(getattr(device, "arm_processing_activation", None)) for device in board.devices):
        from emulator import image_processing_activation
        modules.add("emulator.image_processing_activation")
    if any(type(device).__module__ == "emulator.jpeg_encode"
           and callable(getattr(device, "arm_next_initialization", None)) for device in board.devices):
        from emulator import jpeg_successor
        modules.add("emulator.jpeg_successor")
    if any(type(device).__module__ == "emulator.jpeg_encode"
           and callable(getattr(device, "arm_declared_stream_encode", None)) for device in board.devices):
        from emulator import jpeg_streamed
        modules.add("emulator.jpeg_streamed")
    if any(type(device).__module__ == "emulator.input_processing_devices"
           and callable(getattr(device, "arm_processing_activation", None)) for device in board.devices):
        from emulator import input_processing_activation
        modules.add("emulator.input_processing_activation")
    if any(type(device).__module__ == "emulator.input_processing_devices"
           and callable(getattr(device, "arm_processing_output", None)) for device in board.devices):
        from emulator import processing_image_delivery, processing_image_scene
        modules.update(("emulator.processing_image_delivery", "emulator.processing_image_scene"))
    for api, helper in (("arm_next_initialization", "still_resize_successor"),
                        ("arm_preview_scene", "still_resize_preview"),
                        ("arm_thumbnail_scene", "still_resize_thumbnail"),
                        ("arm_jpeg_stream_initialization", "still_resize_jpeg_stream"),
                        ("arm_jpeg_main_stream_initialization", "still_resize_jpeg_main_stream")):
        if any(type(device).__module__ == "emulator.still_resize_devices"
               and callable(getattr(device, api, None)) for device in board.devices):
            __import__("emulator." + helper)
            modules.add("emulator." + helper)
    if any(type(device).__module__ == "emulator.image_work_devices"
           and callable(getattr(device,"arm_processing_reset",None)) for device in board.devices):
        from emulator import image_processing_reset
        modules.add("emulator.image_processing_reset")
    if any(type(device).__module__ == "emulator.image_work_devices"
           and callable(getattr(device,"arm_processing_configuration",None)) for device in board.devices):
        from emulator import image_processing_configuration
        modules.add("emulator.image_processing_configuration")
    if any(type(device).__module__ in ("emulator.afs_frame_source", "emulator.producer_devices")
           and callable(getattr(device,"startup_scope",None)) for device in board.devices):
        from emulator import afs_startup_scope
        modules.add("emulator.afs_startup_scope")
    if any(type(device).__module__ == "emulator.input_processing_devices"
           and callable(getattr(device,"arm_still_return_phases",None)) for device in board.devices):
        from emulator import input_still_return
        modules.add("emulator.input_still_return")
    if any(type(device).__module__ == "emulator.input_processing_devices"
           and callable(getattr(device, "arm_still_raw", None)) for device in board.devices):
        from emulator import afs_still_readout, afs_still_raw, afs_raw_source
        modules.update(("emulator.afs_still_readout", "emulator.afs_still_raw", "emulator.afs_raw_source"))
    if any(type(device).__module__ == "emulator.producer_devices"
           and callable(getattr(device, "arm_afs_focus_statistics", None)) for device in board.devices):
        from emulator import afs_focus_data, afs_focus_scene
        modules.update(("emulator.afs_focus_data", "emulator.afs_focus_scene"))
    if board._portable_factory_options.get("af_extraction_profile") is not None:
        from emulator import afs_scene_fixture
        modules.add("emulator.afs_scene_fixture")
    if board._portable_factory_options.get("afs_frame_source_profile") is not None:
        from emulator import afs_statistics_frame
        modules.add("emulator.afs_statistics_frame")
    if board._portable_factory_options.get("still_pipeline_profile") is not None:
        from emulator import still_processing_scene, jpeg_declared, jpeg_codec
        modules.update(("emulator.still_processing_scene", "emulator.jpeg_declared", "emulator.jpeg_codec"))
        if any(type(device).__module__ == "emulator.still_processing_devices"
               and callable(getattr(device, "arm_initialization", None)) for device in board.devices):
            from emulator import still_processing_prelude
            modules.add("emulator.still_processing_prelude")
    if board._portable_factory_options.get("settings") == "owner":
        from emulator import owner_settings
        modules.add("emulator.owner_settings")
    return dict(python=[sys.implementation.name, list(sys.version_info[:3])],
        native={key: native[key] for key in ("version", "target", "pyo3", "extension_sha256",
            "unicorn_abi", "unicorn_library_sha256", "loaded_adapters")},
        models={name: _loaded_module(sys.modules[name])["sha256"] for name in sorted(modules)},
        value_classes={name: _class(cls)["sha256"] for name, cls in value_classes.items()},
        factory=_callback_identity(boot.make_board),
        architecture_code=_loaded_module(architecture)["sha256"],
        portable_code=_loaded_module(sys.modules[__name__])["sha256"],
        firmware_sha256=board.manifest["source_sha256"], rom_sha256=_sha(board.rom))


class _Writer:
    def __init__(self, directory):
        self.directory = directory
        self.blobs = {}

    def blob(self, data):
        data = bytes(data)
        digest = _sha(data)
        if digest not in self.blobs:
            compressed = gzip.compress(data, mtime=0)
            (self.directory / (digest + ".gz")).write_bytes(compressed)
            self.blobs[digest] = dict(bytes=len(data), compressed_bytes=len(compressed),
                                      compressed_sha256=_sha(compressed))
        return {"blob": digest}


class _Reader:
    def __init__(self, directory, blobs):
        self.directory, self.blobs, self.cache = directory, blobs, {}

    def blob(self, value):
        digest = value["blob"]
        if (not isinstance(digest, str) or len(digest) != 64
                or any(c not in "0123456789abcdef" for c in digest) or digest not in self.blobs):
            raise PortableCheckpointError("Unknown or invalid blob reference")
        if digest not in self.cache:
            metadata = self.blobs[digest]
            length = metadata["bytes"]
            if type(length) is not int or not 0 <= length <= 1 << 30:
                raise PortableCheckpointError("Blob length exceeds format bound")
            path = self.directory / (digest + ".gz")
            if path.is_symlink() or path.stat().st_size != metadata["compressed_bytes"]:
                raise PortableCheckpointError("Compressed blob size/path differs")
            compressed = path.read_bytes()
            if _sha(compressed) != metadata["compressed_sha256"]:
                raise PortableCheckpointError("Compressed blob SHA-256 differs")
            with gzip.open(path, "rb") as source:
                data = source.read(length + 1)
            if len(data) != length or _sha(data) != digest:
                raise PortableCheckpointError("Blob length/SHA-256 differs")
            self.cache[digest] = data
        return self.cache[digest]


class _Encoder:
    def __init__(self, board, writer):
        self.board, self.writer = board, writer
        self.references = {id(board): "board"}
        self.references.update({id(device): index for index, device in enumerate(board.devices)})
        self.nodes, self.memo = [], {}
        self.classes = _value_classes()
        self.enums = _enum_classes()

    def encode(self, value, owner=None, field=None):
        if id(value) in self.references:
            return {"reference": self.references[id(value)]}
        if value is None or type(value) in (str, int, bool):
            return value
        if _name(type(value)) in self.enums:
            return {"enum": [_name(type(value)), value.value]}
        if type(value) is float:
            if not math.isfinite(value):
                raise PortableCheckpointError("Nonfinite model value")
            return {"float": value.hex()}
        if type(value) is bytes:
            return self.writer.blob(value)
        if isinstance(value, Path):
            return {"path": str(value)}
        identifier = id(value)
        if identifier in self.memo:
            return {"node": self.memo[identifier]}
        if len(self.nodes) >= MAX_NODES:
            raise PortableCheckpointError("Model graph exceeds format bound")
        index = len(self.nodes)
        self.memo[identifier] = index
        node = {}
        self.nodes.append(node)
        if isinstance(value, MethodType):
            target = self.references.get(id(value.__self__))
            expected = getattr(self.board, FACTORY_METHODS.get(field, ""), None)
            if (target != "board" or owner is None or field is None or expected is None
                    or _callback_identity(value) != _callback_identity(expected)):
                raise PortableCheckpointError("Callback owner is not this board/device")
            node.update(kind="method", owner=target, method=value.__name__,
                        field_owner=owner, field=field, identity=_callback_identity(value))
        elif isinstance(value, FunctionType):
            # Only factory-created direct device fields may retain lambdas.
            if (owner is None or field is None or value.__module__ != "emulator.boot"
                    or not value.__qualname__.startswith("make_board.<locals>.<lambda>")
                    or value.__defaults__ is not None or value.__kwdefaults__ is not None
                    or value.__code__.co_freevars != ("board",)
                    or len(value.__closure__ or ()) != 1
                    or value.__closure__[0].cell_contents is not self.board):
                raise PortableCheckpointError("Unrecognized executable callback in model state")
            from emulator.boot import make_board
            if not any(value.__code__ is constant for constant in make_board.__code__.co_consts):
                raise PortableCheckpointError("Callback code is not retained by the stock factory")
            node.update(kind="factory_callback", owner=owner, field=field, identity=_callback_identity(value))
        elif type(value) is bytearray:
            node.update(kind="bytearray", data=self.writer.blob(value))
        elif isinstance(value, Mapping):
            if type(value) not in (dict, MappingProxyType):
                raise PortableCheckpointError("Unsupported mapping type " + _name(type(value)))
            node.update(kind="mappingproxy" if type(value) is MappingProxyType else "dict",
                items=[[self.encode(k), self.encode(v)] for k, v in value.items()])
        elif type(value) in (list, tuple, set, frozenset, deque):
            node.update(kind=type(value).__name__, items=[self.encode(item) for item in value])
            if type(value) is deque:
                node["maxlen"] = value.maxlen
        elif _name(type(value)) in self.classes:
            names = [f.name for f in fields(value)] if is_dataclass(value) else VALUE_FIELDS[_name(type(value))]
            if not is_dataclass(value) and set(vars(value)) != set(names):
                raise PortableCheckpointError(type(value).__name__ + " field schema differs")
            _validate_value(value)
            node.update(kind="value", cls=_name(type(value)), value_schema=1,
                        fields={name: self.encode(getattr(value, name)) for name in names})
        else:
            raise PortableCheckpointError("External or unsupported model state " + _name(type(value)))
        return {"node": index}

    def state(self, state, owner):
        return {name: self.encode(value, owner, name) for name, value in state.items()}


class _Decoder:
    def __init__(self, board, reader, nodes):
        if type(nodes) is not list or len(nodes) > MAX_NODES:
            raise PortableCheckpointError("Invalid model graph")
        self.board, self.reader, self.nodes = board, reader, nodes
        self.cache, self.active = {}, set()
        self.classes = _value_classes()
        self.enums = _enum_classes()

    def owner(self, value):
        if value == "board":
            return self.board
        if type(value) is not int or not 0 <= value < len(self.board.devices):
            raise PortableCheckpointError("Invalid device reference")
        return self.board.devices[value]

    def decode(self, value):
        if value is None or type(value) in (str, int, bool):
            return value
        if type(value) is not dict or len(value) != 1:
            raise PortableCheckpointError("Invalid typed model value")
        kind, item = next(iter(value.items()))
        if kind == "enum":
            if (type(item) is not list or len(item) != 2 or type(item[0]) is not str
                    or item[0] not in self.enums or type(item[1]) not in (int, str)):
                raise PortableCheckpointError("Invalid trusted model enum")
            try:
                return self.enums[item[0]](item[1])
            except ValueError as error:
                raise PortableCheckpointError("Unknown trusted model enum value") from error
        if kind == "reference":
            return self.owner(item)
        if kind == "blob":
            return self.reader.blob(value)
        if kind == "float":
            result = float.fromhex(item)
            if not math.isfinite(result):
                raise PortableCheckpointError("Nonfinite model value")
            return result
        if kind == "path":
            return Path(item)
        if kind != "node" or type(item) is not int or not 0 <= item < len(self.nodes):
            raise PortableCheckpointError("Invalid model node reference")
        if item in self.cache:
            return self.cache[item]
        if item in self.active:
            raise PortableCheckpointError("Cyclic immutable model node")
        self.active.add(item)
        node = self.nodes[item]
        kind = node["kind"]
        if kind in ("method", "factory_callback"):
            owner = self.owner(node["field_owner"] if kind == "method" else node["owner"])
            result = getattr(owner, node["field"], None)
            if not callable(result) or _callback_identity(result) != node["identity"]:
                raise PortableCheckpointError("Factory callback identity differs")
            if kind == "method" and (not isinstance(result, MethodType)
                    or result.__self__ is not self.owner(node["owner"])
                    or result.__name__ != node["method"]):
                raise PortableCheckpointError("Factory callback binding differs")
        elif kind == "bytearray":
            result = bytearray(self.reader.blob(node["data"]))
        elif kind in ("dict", "mappingproxy"):
            result = {}
            self.cache[item] = result
            for key, val in node["items"]:
                decoded_key = self.decode(key)
                if decoded_key in result:
                    raise PortableCheckpointError("Duplicate model mapping key")
                result[decoded_key] = self.decode(val)
            if kind == "mappingproxy":
                result = MappingProxyType(result)
        elif kind in ("list", "tuple", "set", "frozenset", "deque"):
            if kind == "list":
                result = []
                self.cache[item] = result
                result.extend(self.decode(v) for v in node["items"])
            else:
                values = [self.decode(v) for v in node["items"]]
                result = {"tuple": tuple, "set": set, "frozenset": frozenset,
                          "deque": lambda v: deque(v, maxlen=node["maxlen"])}[kind](values)
        elif kind == "value" and node["cls"] in self.classes:
            cls = self.classes[node["cls"]]
            names = {f.name for f in fields(cls)} if is_dataclass(cls) else set(VALUE_FIELDS[node["cls"]])
            if type(node.get("value_schema")) is not int or node["value_schema"] != 1 or set(node["fields"]) != names:
                raise PortableCheckpointError("Value class field schema differs")
            result = object.__new__(cls)
            self.cache[item] = result
            for name, value in node["fields"].items():
                object.__setattr__(result, name, self.decode(value))
            _validate_value(result)
        else:
            raise PortableCheckpointError("Unknown model node kind/class")
        self.cache[item] = result
        self.active.remove(item)
        return result

    def state(self, state):
        if type(state) is not dict or any(type(name) is not str for name in state):
            raise PortableCheckpointError("Invalid model field dictionary")
        return {name: self.decode(value) for name, value in state.items()}


def _require_native_portable(board):
    api = getattr(board._native, "portable_checkpoint_status", None)
    if not callable(api):
        raise PortableCheckpointError("Native extension lacks portable checkpoint eligibility tracking")
    status = api()
    if (type(status) is not dict or set(status) != {"contract", "supported", "reasons"}
            or status["contract"] != "lifetime-arm32-rust-state-v1"
            or type(status["supported"]) is not bool or type(status["reasons"]) is not list
            or any(type(reason) is not str or not reason for reason in status["reasons"])):
        raise PortableCheckpointError("Native portable eligibility contract differs")
    if not status["supported"] or status["reasons"]:
        raise PortableCheckpointError("Unexported native state: " + "; ".join(status["reasons"]))
    return status


def _preflight(board):
    if type(board) is not RustBoard:
        raise PortableCheckpointError("Portable checkpoints require a stock RustBoard")
    _require_native_modes_disabled(board)
    reference._paused(board, capturing=True)
    if not isinstance(getattr(board, "_portable_factory_options", None), dict):
        raise PortableCheckpointError("Board lacks stock factory reconstruction options")
    selected = board._portable_factory_options.get("firmware_profile")
    if selected is None:
        from emulator.board import ROM_HASH
        rom_hash = ROM_HASH
    else:
        profile = _resolve_factory_profile(selected)
        rom_hash = profile.rom_hash
        if (board.manifest != profile.manifest or board.nand.flash != profile.flash):
            raise PortableCheckpointError("Selected firmware manifest or native flash backing differs")
    if _sha(board.rom) != rom_hash or bytes(board.rom_storage) != board.rom:
        raise PortableCheckpointError("Portable checkpoint refuses changes to the pinned ROM backing")
    if getattr(board, "mmio_policy", "strict") != "strict" or getattr(board, "exploration", None) is not None:
        raise PortableCheckpointError("Portable checkpoint requires strict MMIO without exploration")
    names = list(STOCK_DEVICE_NAMES)
    from emulator.lens_homing_fixture import HOMING_PROFILE
    if board._portable_factory_options.get("lens_response_profile") == HOMING_PROFILE:
        names.insert(9, "LensHomingFixture")
    if board._portable_factory_options.get("sd_card_profile") is not None:
        names.insert(1, "SDCardController")
    if board._portable_factory_options.get("wifi_sdio_profile") is not None:
        names.insert(2 if board._portable_factory_options.get("sd_card_profile") is not None else 1,
                     "WiFiSDIOController")
    if board._portable_factory_options.get("af_extraction_profile") is not None:
        names.insert(names.index("HDMIStartup"), "AFExtractionControl")
    if board._portable_factory_options.get("afs_frame_source_profile") is not None:
        names.insert(names.index("HDMIStartup"), "AfsFrameSource")
    if board._portable_factory_options.get("still_pipeline_profile") is not None:
        names.extend(("StillProcessingControl", "JPEGEncodeControl"))
    from emulator import boot
    if (board._portable_factory_options.get("still_pipeline_profile") is not None
            and board._portable_factory_options.get("still_pipeline_profile") ==
                getattr(boot,"STILL_PIPELINE_RESIZE_PROFILE",None)):
        names.append("StillResizeControl")
    if (len(board.devices) != len(names)
            or any(type(device) is not getattr(boot, name) for name, device in zip(names, board.devices))):
        raise PortableCheckpointError("Portable checkpoint requires the complete stock device topology")
    for device in board.devices:
        if type(device) is boot.BootControls:
            _validate_acquisition_state(vars(device), board.virtual_ticks)
    return _require_native_portable(board)


def save(board, directory):
    """Save to a NEW directory while the caller holds the session pause lock."""
    eligibility = _preflight(board)
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=False)
    try:
        writer = _Writer(directory)
        encoder = _Encoder(board, writer)
        model = {"board": encoder.state(reference._board_state(board), "board"),
                 "devices": [encoder.state(vars(device), index) for index, device in enumerate(board.devices)]}
        model["nodes"] = encoder.nodes
        state = _json(model)
        if len(state) > MAX_STATE_BYTES:
            raise PortableCheckpointError("Model state exceeds format bound")
        compressed = gzip.compress(state, mtime=0)
        (directory / "model.json.gz").write_bytes(compressed)
        ram_size, pages, ram_hash = reference._sparse_ram(board)
        architecture = [export_cpu(cpu) for cpu in board.cpus]
        for cpu_state in architecture:
            validate_cpu_state(cpu_state)
        manifest = dict(schema=SCHEMA, identity=_identity(board), native_eligibility=eligibility,
            factory_options=board._portable_factory_options,
            device_classes=[_name(type(device)) for device in board.devices],
            model=dict(bytes=len(state), sha256=_sha(state), compressed_bytes=len(compressed),
                       compressed_sha256=_sha(compressed)),
            ram=dict(bytes=ram_size, sha256=ram_hash,
                     pages=[[offset, writer.blob(data)] for offset, data in pages.items()]),
            sram=writer.blob(bytes(board.sram)), vectors=writer.blob(bytes(board.vector_scratch)),
            private=[[[address, writer.blob(data)] for address, data in ranges]
                     for ranges in reference._private_memory(board)],
            mappings=reference._mapping_layout(board),
            private_ranges=[reference._private_ranges(board, core) for core in range(len(board.cpus))],
            low_rom_alias=board.low_rom_alias,
            architecture=architecture,
            fingerprint=reference.fingerprint(board), blobs=writer.blobs)
        (directory / "manifest.json").write_bytes(_json(manifest) + b"\n")
        return manifest
    except BaseException:
        shutil.rmtree(directory)
        raise


def load(directory, *, instructions_per_tick=None, override_ratio=False):
    """Create fresh stock engines and reconstruct a saved drained boundary.

    A snapshot records its declared CPU-speed ratio (``instructions_per_tick``
    factory option; absent means 1). Passing a different ratio is refused
    unless ``override_ratio`` is true, which then changes it at the restored
    run boundary (recorded in the board events).
    """
    try:
        return _load(directory, instructions_per_tick=instructions_per_tick, override_ratio=override_ratio)
    except PortableCheckpointError:
        raise
    except (ArchitectureError, KeyError, ValueError, TypeError, OSError, AttributeError, OverflowError) as error:
        raise PortableCheckpointError(f"Invalid or unavailable portable state: {error}") from error


def load_for_development(directory, revisions, *, instructions_per_tick=None, override_ratio=False):
    """Resume unchanged state under explicitly reviewed code-only revisions.

    Each key is ``models.<module>``, ``portable_code`` or
    ``factory.code_sha256`` and each value is
    ``{"saved": <sha256>, "current": <sha256>}``. The list must describe
    every changed digest exactly. Only the newly curated
    preview-statistics, producer-measurement, measurement-data, measurement-scene,
    measurement-iris, AF-motion, contrast-AF producer/clock/data/scene,
    preview-return/capture clocks, capture metering and still RAW helpers
    can use ``saved: null``, and
    only when its key is absent from the saved identity. The current digest is
    still pinned exactly. The factory's module/name/options, native,
    architecture, firmware, callbacks, topology and the full restored state
    fingerprint still match. A factory code revision is not a profile or
    device-field migration; its reconstruction must be reviewed separately.
    This checks reconstruction, not behavioral equivalence of revised models;
    qualify their new transactions separately and periodically replay ROM boot.
    """
    try:
        return _load(directory, revisions=revisions, instructions_per_tick=instructions_per_tick,
                     override_ratio=override_ratio)
    except PortableCheckpointError:
        raise
    except (ArchitectureError, KeyError, ValueError, TypeError, OSError, AttributeError, OverflowError) as error:
        raise PortableCheckpointError(f"Invalid or unavailable portable state: {error}") from error


def _check_identity(current, saved, revisions):
    if revisions is None:
        if current != saved:
            raise PortableCheckpointError("Loaded model/native/firmware identity differs")
        return
    if type(revisions) is not dict or not revisions:
        raise PortableCheckpointError("Development revisions must be an explicit nonempty digest map")
    permitted = {"portable_code", "factory.code_sha256"} | {"models." + name for name in current["models"]}
    revised = dict(saved)
    revised["models"] = dict(saved["models"])
    revised["factory"] = dict(saved["factory"])
    for selector, pair in revisions.items():
        if selector not in permitted or type(pair) is not dict or set(pair) != {"saved", "current"}:
            raise PortableCheckpointError("Unknown development revision selector/digest schema")
        name = selector.removeprefix("models.")
        helper_addition = (selector in ("models.emulator.preview_statistics", "models.emulator.producer_measurement",
                                       "models.emulator.afs_measurement_data", "models.emulator.measurement_scene",
                                       "models.emulator.lens_measurement_iris", "models.emulator.lens_af_motion",
                                       "models.emulator.producer_afs_focus", "models.emulator.lens_focus_motion",
                                       "models.emulator.afs_contrast_clock", "models.emulator.afs_focus_data",
                                       "models.emulator.afs_focus_scene", "models.emulator.afs_preview_return_clock",
                                       "models.emulator.afs_capture_clock", "models.emulator.afs_capture_start_clock", "models.emulator.afs_capture_stop_clock", "models.emulator.producer_capture_metstats",
                                       "models.emulator.producer_still_capture", "models.emulator.afs_still_initialization_clock", "models.emulator.afs_still_qualifying_clock", "models.emulator.afs_still_readout_clock", "models.emulator.afs_still_tail_clock", "models.emulator.afs_still_image_clock", "models.emulator.afs_still_readout", "models.emulator.afs_still_raw",
                                       "models.emulator.afs_raw_source", "models.emulator.image_still_reset", "models.emulator.image_still_configuration", "models.emulator.image_still_prepare", "models.emulator.image_still_activation", "models.emulator.afs_still_image_delivery", "models.emulator.afs_still_image", "models.emulator.afs_still_numeric", "models.emulator.still_processing_prelude", "models.emulator.afs_still_cleanup_clock", "models.emulator.face_still_reconfiguration", "models.emulator.face_still_cancellation", "models.emulator.afs_startup_scope", "models.emulator.prefilter_initialization", "models.emulator.prefilter_active", "models.emulator.prefilter_scene", "models.emulator.prefilter_second", "models.emulator.input_still_return", "models.emulator.prefilter_third", "models.emulator.prefilter_empty", "models.emulator.still_f1c_idle", "models.emulator.producer_processing_reset", "models.emulator.image_processing_reset", "models.emulator.still_f1c_processing", "models.emulator.still_f1c_scene", "models.emulator.producer_processing_configuration", "models.emulator.image_processing_configuration", "models.emulator.still_f1c_gain", "models.emulator.processing_passive_configuration", "models.emulator.processing_control_configuration", "models.emulator.processing_pipeline_reset", "models.emulator.still_resize_successor", "models.emulator.still_resize_preview", "models.emulator.still_resize_thumbnail", "models.emulator.jpeg_successor", "models.emulator.image_processing_activation", "models.emulator.still_resize_jpeg_stream", "models.emulator.jpeg_streamed", "models.emulator.input_processing_activation", "models.emulator.still_resize_jpeg_main_stream", "models.emulator.processing_image_delivery", "models.emulator.processing_image_scene")
            and name in IDENTITY_HELPER_MODULES and name not in saved["models"]
            and name in current["models"] and pair["saved"] is None)
        digests = (pair["current"],) if helper_addition else pair.values()
        if any(type(digest) is not str or len(digest) != 64
               or any(c not in "0123456789abcdef" for c in digest) for digest in digests):
            raise PortableCheckpointError("Invalid development revision digest")
        if selector == "factory.code_sha256":
            old, new = saved["factory"]["code_sha256"], current["factory"]["code_sha256"]
        else:
            old = saved["models"].get(name) if selector.startswith("models.") else saved[selector]
            new = current["models"][name] if selector.startswith("models.") else current[selector]
        if old == new or pair != {"saved": old, "current": new}:
            raise PortableCheckpointError("Development revision does not match changed saved/current digests")
        if selector == "factory.code_sha256":
            revised["factory"]["code_sha256"] = new
        elif selector.startswith("models."):
            revised["models"][name] = new
        else:
            revised[selector] = new
    if revised != current:
        raise PortableCheckpointError("Unlisted model/native/firmware identity differs")


def _load(directory, *, revisions=None, instructions_per_tick=None, override_ratio=False):
    from emulator.boot import make_board
    directory = Path(directory)
    manifest_path = directory / "manifest.json"
    if manifest_path.is_symlink() or manifest_path.stat().st_size > MAX_STATE_BYTES:
        raise PortableCheckpointError("Invalid manifest path/size")
    manifest = json.loads(manifest_path.read_bytes())
    if manifest.get("schema") != SCHEMA:
        raise PortableCheckpointError("Unsupported portable checkpoint schema")
    options = manifest["factory_options"]
    signature = inspect.signature(make_board)
    # Optional selections deliberately omit their unselected keys from saved
    # options. No selected peripheral or owner settings are added on restore.
    optional_absent = {"still_pipeline_profile", "instructions_per_tick", "settings", "firmware_profile"} & set(signature.parameters)
    if (type(options) is not dict or not (set(options) <= set(signature.parameters) and set(signature.parameters) - set(options) <= optional_absent)
            or any(value is not None and type(value) not in (str, int, bool, float)
                   for key, value in options.items() if key != "firmware_profile")):
        raise PortableCheckpointError("Factory option schema differs")
    reconstruction = dict(options)
    if options.get("firmware_profile") is not None:
        reconstruction["firmware_profile"] = _resolve_factory_profile(options["firmware_profile"])
    from .board import normalize_instructions_per_tick
    saved_ratio = normalize_instructions_per_tick(options.get("instructions_per_tick", 1))
    wanted_ratio = saved_ratio if instructions_per_tick is None else normalize_instructions_per_tick(instructions_per_tick)
    if wanted_ratio != saved_ratio and not override_ratio:
        raise PortableCheckpointError(
            f"Snapshot was saved with instructions_per_tick={saved_ratio[0]}/{saved_ratio[1]}; "
            f"refusing {wanted_ratio[0]}/{wanted_ratio[1]} without override_ratio")
    board = make_board(**reconstruction)
    if _json(board._portable_factory_options) != _json(options):
        raise PortableCheckpointError("Factory reconstruction options differ")
    if _require_native_portable(board) != manifest["native_eligibility"]:
        raise PortableCheckpointError("Saved native portable eligibility differs")
    if type(board) is not RustBoard:
        raise PortableCheckpointError("Loaded board type differs")
    _check_identity(_identity(board), manifest["identity"], revisions)
    if [_name(type(device)) for device in board.devices] != manifest["device_classes"]:
        raise PortableCheckpointError("Stock device topology differs")
    for cpu,state in zip(board.cpus,manifest['architecture']):
        validate_cpu_state(state)
        import_tcm(cpu,state['tcm'])
    if (json.loads(_json(reference._mapping_layout(board))) != manifest["mappings"]
            or json.loads(_json([reference._private_ranges(board, c) for c in range(len(board.cpus))])) != manifest["private_ranges"]
            or ctypes.sizeof(board.ram) != manifest["ram"]["bytes"]):
        raise PortableCheckpointError("Stock memory mapping/backing layout differs")
    reader = _Reader(directory, manifest["blobs"])
    model_path = directory / "model.json.gz"
    metadata = manifest["model"]
    if (model_path.is_symlink() or model_path.stat().st_size != metadata["compressed_bytes"]
            or metadata["bytes"] > MAX_STATE_BYTES):
        raise PortableCheckpointError("Model state size/path differs")
    if _sha(model_path.read_bytes()) != metadata["compressed_sha256"]:
        raise PortableCheckpointError("Compressed model SHA-256 differs")
    with gzip.open(model_path, "rb") as source:
        data = source.read(metadata["bytes"] + 1)
    if len(data) != metadata["bytes"] or _sha(data) != metadata["sha256"]:
        raise PortableCheckpointError("Model state length/SHA-256 differs")
    model = json.loads(data)
    decoder = _Decoder(board, reader, model["nodes"])
    if set(model["board"]) & (reference.BOARD_IDENTITIES | reference.DERIVED_FIELDS):
        raise PortableCheckpointError("Model state contains native backing identities or derived caches")
    python = decoder.state(model["board"])
    if _json(python.get("_portable_factory_options")) != _json(options):
        raise PortableCheckpointError("Factory options differ from saved model state")
    devices = [decoder.state(state) for state in model["devices"]]
    if len(devices) != len(board.devices):
        raise PortableCheckpointError("Device state count differs")
    from emulator.boot_devices import BootControls
    for device, state in zip(board.devices, devices):
        if type(device) is BootControls:
            _validate_acquisition_state(state, python.get("virtual_ticks"))
    if revisions is not None:
        for device, state in zip(board.devices, devices):
            if not set(vars(device)) <= set(state):
                raise PortableCheckpointError("Development revision requires a device field migration: " + _name(type(device)))
    if len(manifest["architecture"]) != len(board.cpus):
        raise PortableCheckpointError("Architectural CPU count differs")
    for state in manifest["architecture"]:
        validate_cpu_state(state)
    # Resolve all callbacks before clearing any fresh factory field dictionaries.
    # Fresh RAM is zero except constructor-loaded sections. Clear those pages
    # without touching the entire 512 MiB backing in every restored process.
    _fresh_size, fresh_pages, _fresh_hash = reference._sparse_ram(board)
    for offset, raw in fresh_pages.items():
        ctypes.memset(board.ram_pointer + offset, 0, len(raw))
    last = -1
    for offset, blob in manifest["ram"]["pages"]:
        raw = reader.blob(blob)
        if (type(offset) is not int or offset <= last or offset % PAGE_SIZE
                or offset + len(raw) > ctypes.sizeof(board.ram)
                or len(raw) != min(PAGE_SIZE, ctypes.sizeof(board.ram) - offset)):
            raise PortableCheckpointError("Invalid sparse RAM page")
        ctypes.memmove(board.ram_pointer + offset, raw, len(raw))
        last = offset
    for backing, blob in ((board.sram, manifest["sram"]), (board.vector_scratch, manifest["vectors"])):
        raw = reader.blob(blob)
        if len(raw) != ctypes.sizeof(backing):
            raise PortableCheckpointError("Shared backing length differs")
        ctypes.memmove(ctypes.addressof(backing), raw, len(raw))
    if len(manifest["private"]) != len(board.cpus):
        raise PortableCheckpointError("Private CPU backing count differs")
    for core, (cpu, ranges) in enumerate(zip(board.cpus, manifest["private"])):
        expected = reference._private_ranges(board, core)
        if len(ranges) != len(expected):
            raise PortableCheckpointError("Private CPU range count differs")
        for (address, blob), (wanted_address, length) in zip(ranges, expected):
            raw = reader.blob(blob)
            if address != wanted_address or len(raw) != length:
                raise PortableCheckpointError("Private backing range differs")
            cpu.mem_write(address, raw)
    if board.low_rom_alias != manifest["low_rom_alias"]:
        board._remap_low_alias(manifest["low_rom_alias"])
    preserved = reference.BOARD_IDENTITIES | reference.DERIVED_FIELDS
    for name in list(vars(board)):
        if name not in preserved:
            del board.__dict__[name]
    board.__dict__.update(python)
    for device, state in zip(board.devices, devices):
        device.__dict__.clear()
        device.__dict__.update(state)
    for cpu, state in zip(board.cpus, manifest["architecture"]):
        import_cpu(cpu, state)
        if export_cpu(cpu) != state:
            raise PortableCheckpointError("Restored explicit CPU/CP15/MPU state differs")
    board.invalidate_execution_caches()
    _preflight(board)
    if reference.fingerprint(board) != manifest["fingerprint"]:
        raise PortableCheckpointError("Full restored memory/model/architecture fingerprint differs")
    if wanted_ratio != saved_ratio:
        board.set_instructions_per_tick(wanted_ratio)
    return board
