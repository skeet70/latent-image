"""Strict image-engine scanout from Board RAM and stock MMIO writes.

No cfgdata, RTOS callback, screen contents or producer addresses are supplied.
The first rendering subset is a single main video plane at 1:1 scale. Graphics
registers are retained and exposed, but enabled unsupported planes make frame
rendering fail explicitly. See research/emulator-scanout/README.md.
"""
from dataclasses import asdict, dataclass
from enum import Enum
import hashlib
import struct
from typing import NewType

import numpy as np

from .display import PACKINGS, MATRICES, render_rgb, encode_png
from .hardware import UnsupportedDevice


class UnsupportedScanout(UnsupportedDevice):
    """The CPU may continue; requested pixel composition is not modeled."""

Pixel = NewType("Pixel", int)
RectangleIndex = NewType("RectangleIndex", int)
Luma = NewType("Luma", int)
Opacity = NewType("Opacity", int)


class RectangleProfile(str, Enum):
    XM1 = "xm1-150-declared-luma-rectangle-overlay-unmeasured"


@dataclass(frozen=True)
class RectangleBounds:
    left: Pixel
    top: Pixel
    right: Pixel
    bottom: Pixel


@dataclass(frozen=True)
class RectangleOverlay:
    index: RectangleIndex
    bounds: RectangleBounds
    thickness: Pixel
    luma: Luma
    opacity: Opacity



@dataclass
class ScanoutFrame:
    raw: bytes
    rgb: np.ndarray | None  # None for a metadata-only frame (pixels=False)
    png: bytes | None
    state: dict


class DisplayScanout:
    BASES = (0xfff6b000, 0xfff6e000)
    # Exact image-register offsets retained by 0x961c14/committed by0x962d54.
    OFFSETS = (set(range(0, 0x104, 4)) - {0x18, 0x104}) | set(range(0x108, 0x19c, 4)) | {0x1bc, 0x9d0, 0x9d4}
    MAIN_REQUIRED = {0, 0x10, 0x24, 0x28, 0x2c, 0x30, 0xd4, 0xd8, 0xdc}
    GRAPHICS_PROFILES = {"mode1-bgra8888-straight", "mode1-bgra8888-premultiplied"}
    BLACK_BACKDROP_PROFILE = "declared-black-canvas-unmeasured"
    BACKDROP_PROFILES = (BLACK_BACKDROP_PROFILE,)

    def __init__(self, *, read_bytes, schedule, write_bytes=None, record=None, packing="yycbcr",
                 matrix="bt601-full", graphics_profile=None, backdrop_profile=None, rectangle_profile=None,
                 ram_bytes=0x20000000, max_pixels=4096 * 2160):
        if packing not in PACKINGS or matrix not in MATRICES:
            raise ValueError("Unsupported explicit scanout color interpretation")
        if graphics_profile is not None and graphics_profile not in self.GRAPHICS_PROFILES:
            raise ValueError("Unknown explicit graphics interpretation profile")
        if backdrop_profile is not None and backdrop_profile not in self.BACKDROP_PROFILES:
            raise ValueError("Unknown explicit display backdrop profile")
        if rectangle_profile is not None and type(rectangle_profile) is not RectangleProfile:
            raise ValueError("Rectangle interpretation requires a RectangleProfile")
        self.rectangle_profile = rectangle_profile
        self.read_bytes, self.schedule = read_bytes, schedule
        self.record = record if record is not None else lambda *a, **kw: None
        self.packing, self.matrix = packing, matrix
        self.graphics_profile = graphics_profile
        self.backdrop_profile = backdrop_profile
        self.ram_bytes, self.max_pixels = ram_bytes, max_pixels
        self.regs, self.written = [{}, {}], [set(), set()]
        self.generation = [0, 0]
        self.frames = [0, 0]
        self.reset_completions = [0, 0]
        self.frame_state = [None, None]
        self.render_errors = [None, None]

    def contains(self, address, size):
        return any(base <= address and address + size <= base + 0x1000 for base in self.BASES)

    def _address(self, address, size):
        for instance, base in enumerate(self.BASES):
            offset = address - base
            allowed = self.OFFSETS | ({0x18} if instance else set())
            if 0 <= offset < 0x1000:
                if size != 4 or offset & 3 or offset not in allowed:
                    raise UnsupportedDevice(f"Unknown image-engine register/access {address:#x}/{size}")
                return instance, offset
        raise UnsupportedDevice(f"Unknown image-engine bank {address:#x}")

    def read(self, cpu_id, address, size):
        instance, offset = self._address(address, size)
        return self.regs[instance].get(offset, 0)

    def write(self, cpu_id, address, size, value):
        instance, offset = self._address(address, size)
        value &= 0xffffffff
        previous = self.regs[instance].get(offset, 0)
        self.regs[instance][offset] = value
        self.written[instance].add(offset)
        if offset == 0 and previous & 0x10000 and not value & 0x10000:
            # 0x961b08 writes bit16, clears it, then polls for it to return.
            # No other bit is made ready. Reset values of other registers are
            # not hardware captured, so existing config is retained explicitly.
            self.generation[instance] += 1
            generation = self.generation[instance]
            self.record("Image-engine reset start", instance=instance, cpu=cpu_id)
            def complete():
                if generation == self.generation[instance]:
                    self.regs[instance][0] |= 0x10000
                    self.reset_completions[instance] += 1
                    self.record("Image-engine reset complete", instance=instance, cpu=cpu_id)
            self.schedule(2, complete)

    def _graphics(self, instance):
        registers = self.regs[instance]
        layers = []
        for layer, control, enable, origin, dimensions, source, stride, alpha in (
            (0, 0x34, 0x40, 0x44, 0x54, 0x64, 0x74, 0xe0),
            (1, 0x84, 0x90, 0x94, 0xa4, 0xb4, 0xc4, 0xe4)):
            flags = registers.get(enable, 0)
            channels = []
            for channel in range(4):
                delta = channel * 4
                dimension_word = registers.get(dimensions + delta, 0)
                source_word = registers.get(source + delta, 0)
                channels.append({"channel": channel, "enabled": bool(flags & (1 << (channel * 4))),
                    "configured": {dimensions + delta, source + delta, stride + delta, origin + delta}.issubset(self.written[instance]),
                    "width": ((dimension_word >> 16) & 0x1fff) + 1,
                    "height": (dimension_word & 0x1fff) + 1,
                    "origin_word": hex(registers.get(origin + delta, 0)),
                    "source": hex(source_word & 0x3fffffff), "source_control": source_word >> 30,
                    "padding_units": registers.get(stride + delta, 0) & 0xfff,
                    "alpha_field": (registers.get(alpha, 0) >> (channel * 8)) & 0xff})
            layers.append({"layer": layer, "mode_field": registers.get(control, 0) & 7,
                "control_word": hex(registers.get(control, 0)), "enable_word": hex(flags),
                "channels": channels})
        return layers

    def _read_ram(self, address, count, cpu_id):
        if address < 0 or count < 0 or address + count > self.ram_bytes:
            raise UnsupportedScanout("Display source extends outside modeled RAM")
        # Read the shared physical RAM alias even while CPU low reads mirror ROM.
        data = self.read_bytes(address | 0x40000000, count, cpu_id=cpu_id)
        if len(data) != count:
            raise UnsupportedScanout("Display RAM read was truncated")
        return data

    def capture_graphics_prefixes(self, instance=0, *, byte_count=256, cpu_id=0):
        """Capture bounded actual bytes; no inferred pixel size or composition."""
        if instance not in (0, 1) or not 1 <= byte_count <= 0x100000:
            raise ValueError("Invalid graphics capture instance/length")
        captures = []
        for layer in self._graphics(instance):
            for channel in layer["channels"]:
                if channel["enabled"] and channel["configured"]:
                    address = int(channel["source"], 0)
                    count = min(byte_count, self.ram_bytes - address)
                    data = self._read_ram(address, count, cpu_id)
                    captures.append({"layer": layer["layer"], **channel,
                                     "requested_prefix_bytes": byte_count,
                                     "captured_bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
                                     "raw": data})
        return captures

    def frame_boundary(self, instance=0, *, cpu_id=0, pixels=True):
        """Render the registers visible at this explicit boundary, or no plane.

        A caller must catch UnsupportedScanout as a rendering failure; it does
        not imply that the firmware's CPU or MMIO initialization failed.
        pixels=False reads and hashes the same planes and keeps every register
        check, but skips composition and PNG encoding (no rgb/png/png_sha256,
        and pixel-alpha coverage is not checked); compose() fills them later.
        """
        if instance not in (0, 1):
            raise ValueError("Display instance must be 0 or 1")
        try:
            frame = self._frame(instance, cpu_id, pixels)
        except UnsupportedScanout as exc:
            self.render_errors[instance] = str(exc)
            self.record("Display rendering unsupported", instance=instance, error=str(exc))
            raise
        self.render_errors[instance] = None
        if frame is not None:
            self.frames[instance] += 1
            frame.state["frame_number"] = self.frames[instance]
            self.frame_state[instance] = frame.state
            self.record("Display frame captured", instance=instance, width=frame.state["width"],
                        height=frame.state["height"], frame_sha256=frame.state["frame_sha256"])
        return frame

    def compose(self, instance, *, cpu_id=0):
        """Pixels for the latest frame_boundary(pixels=False), at the same boundary.

        Counts no new frame and records no event: the caller has not run the
        CPU since that boundary, so the frame number and event log stay as a
        pixel frame_boundary would have left them.
        """
        previous = self.frame_state[instance]
        frame = self._frame(instance, cpu_id, True)
        if frame is not None and previous is not None:
            frame.state["frame_number"] = previous["frame_number"]
            if frame.state["frame_sha256"] != previous["frame_sha256"]:
                raise UnsupportedScanout("Display planes changed since their metadata frame")
            self.frame_state[instance] = frame.state
        return frame

    def _frame(self, instance, cpu_id, pixels=True):
        graphics = self._graphics(instance)
        enabled = [(layer, channel) for layer in graphics for channel in layer["channels"]
                   if channel["enabled"]]
        if not enabled:
            return self._finish_frame(self._main_frame(instance, cpu_id, pixels), instance)
        if self.graphics_profile is None:
            raise UnsupportedScanout("Enabled graphics plane: pixel format/palette/alpha composition is not modeled")
        registers = self.regs[instance]
        if 0x10 not in self.written[instance]:
            raise UnsupportedScanout("Missing graphics canvas geometry")
        canvas = registers[0x10]
        width, height = ((canvas >> 16) & 0xfff) + 1, (canvas & 0xfff) + 1
        if canvas & 0xf000f000 or width * height > self.max_pixels:
            raise UnsupportedScanout("Unknown graphics canvas controls or pixel budget")
        main = self._main_frame(instance, cpu_id, pixels)
        # The optional black canvas is a declared composition fixture, not
        # recovered panel behavior or guest framebuffer content. Strict callers
        # still require opaque coverage when actual main video is absent.
        backdrop_used = main is None and self.backdrop_profile == self.BLACK_BACKDROP_PROFILE
        if not pixels:
            rgb = None
        elif main is not None:
            rgb = main.rgb.copy()
        elif backdrop_used:
            rgb = np.zeros((height, width, 3), dtype=np.uint8)
        else:
            rgb = np.empty((height, width, 3), dtype=np.uint8)
        covered = np.full((height, width), main is not None or backdrop_used, dtype=bool)
        raw_parts, plane_states = ([] if main is None else [main.raw]), []
        for layer, channel in enabled:
            if layer["mode_field"] != 1 or int(layer["control_word"], 0) & ~7:
                raise UnsupportedScanout("Explicit BGRA profile supports only graphics mode1 with no unresolved control bits")
            if not channel["configured"] or (0xe0 + layer["layer"] * 4) not in self.written[instance]:
                raise UnsupportedScanout("Missing graphics source/geometry/alpha registers")
            control_offset = 0x34 + layer["layer"] * 0x50
            if control_offset not in self.written[instance] or channel["source_control"]:
                raise UnsupportedScanout("Missing or unresolved graphics format/source controls")
            # Register 0x08 holds two 2-bit scale codes per graphics layer (layer0 bits7:6/5:4,
            # layer1 bits11:10/9:8, written by 0x9628e4 from the 0x968908 attribute block words
            # 3 and 4). Only 0 (1:1) and both codes 1 are modeled. Firmware evidence: the HDMI
            # attribute setup 0x9731b4 passes 1,1 for output modes 2 and 3 and then doubles the
            # window (0xc29ce8/0xc29cea = 2x the 0xc29cf0 source size), 0 otherwise. Which code is
            # horizontal is unresolved, so unequal codes stay unsupported; pixel replication is a
            # declared interpretation (no filtering is modeled).
            orientation_field = (registers.get(0x08, 0) >> (8 if layer["layer"] else 4)) & 0xf
            if orientation_field not in (0, 5):
                raise UnsupportedScanout("Graphics scale codes other than 1:1 or equal 2x require further modeling")
            scale = 2 if orientation_field == 5 else 1
            origin = int(channel["origin_word"], 0)
            x, y = (origin >> 16) & 0x3fff, origin & 0x3fff
            source_width, source_height = channel["width"], channel["height"]
            plane_width, plane_height = source_width * scale, source_height * scale
            if scale != 1 and x < width and y < height:
                # Declared: a replicated plane is clipped to the canvas (the HDMI OSD source is
                # 960x544 for a 1920x1080 canvas; the window set by 0x9731b4 is 2x the 960x540 source).
                plane_width, plane_height = min(plane_width, width - x), min(plane_height, height - y)
            if origin & 0xc000c000 or x + plane_width > width or y + plane_height > height:
                raise UnsupportedScanout("Graphics origin/crop is outside the supported canvas")
            delta = channel["channel"] * 4
            dimensions = registers[0x54 + layer["layer"] * 0x50 + delta]
            stride_word = registers[0x74 + layer["layer"] * 0x50 + delta]
            alpha = channel["alpha_field"]
            if dimensions & 0xe000e000 or stride_word & ~0xfff or alpha > 128:
                raise UnsupportedScanout("Unresolved graphics dimensions/stride/alpha controls")
            if plane_width * plane_height > self.max_pixels:
                raise UnsupportedScanout("Graphics pixel budget exceeded")
            stride = (source_width + channel["padding_units"]) * 4
            count = (source_height - 1) * stride + source_width * 4
            source = int(channel["source"], 0)
            raw = self._read_ram(source, count, cpu_id)
            if not pixels:
                covered[y:y + plane_height, x:x + plane_width] = True
                raw_parts.append(raw)
                plane_states.append({"layer": layer["layer"], **channel,
                    "origin": [x, y], "stride_bytes": stride, "captured_bytes": count,
                    "packing": "BGRA byte order", "sha256": hashlib.sha256(raw).hexdigest(),
                    **({"scale": scale, "scale_interpretation": "declared nearest-neighbour replication"}
                       if scale != 1 else {})})
                continue
            rows = np.ndarray((source_height, source_width, 4), dtype=np.uint8,
                              buffer=raw, strides=(stride, 4, 1))
            if scale != 1:
                rows = rows.repeat(scale, axis=0).repeat(scale, axis=1)[:plane_height, :plane_width]
            foreground = rows[:, :, [2, 1, 0]].astype(np.uint32)
            numerator = rows[:, :, 3].astype(np.uint32) * alpha
            denominator = 255 * 128
            view = rgb[y:y + plane_height, x:x + plane_width]
            visibility = covered[y:y + plane_height, x:x + plane_width]
            opaque = numerator == denominator
            if np.any(~visibility & ~opaque):
                raise UnsupportedScanout("Transparent graphics has no actual initialized backdrop")
            background = view.astype(np.uint32)
            background[~visibility] = 0  # Its weight is zero under verified opaque pixels.
            if self.graphics_profile.endswith("-premultiplied"):
                front = foreground * (alpha * 255)
            else:
                front = foreground * numerator[:, :, None]
            blended = (front + background * (denominator - numerator[:, :, None]) + denominator // 2) // denominator
            view[:] = np.clip(blended, 0, 255).astype(np.uint8)
            visibility[:] = True
            raw_parts.append(raw)
            plane_states.append({"layer": layer["layer"], **channel,
                "origin": [x, y], "stride_bytes": stride, "captured_bytes": count,
                "packing": "BGRA byte order", "sha256": hashlib.sha256(raw).hexdigest(),
                **({"scale": scale, "scale_interpretation": "declared nearest-neighbour replication"}
                   if scale != 1 else {})})
        if not np.all(covered):
            raise UnsupportedScanout("Graphics planes leave canvas pixels without an actual backdrop")
        raw = b"".join(raw_parts)
        state = {"instance": instance, "image_base": hex(self.BASES[instance]),
                 "width": width, "height": height, "graphics_profile": self.graphics_profile,
                 "backdrop_profile": self.backdrop_profile, "declared_backdrop_used": backdrop_used,
                 "main": None if main is None else main.state, "graphics": plane_states,
                 "frame_sha256": hashlib.sha256(raw).hexdigest(),
                 "png_sha256": None,
                 "kind": "actual RAM composition with an explicit graphics interpretation",
                 "limitations": ["Mode1 BGRA is inferred from stock32-bit copy and UI setup, not a hardware color capture",
                                 "Channel alpha128 means full; pixel alpha255 means full; rounding is an explicit arithmetic approximation",
                                 "Ascending group/channel order and selected straight/premultiplied interpretation are profile assumptions",
                                 "Panel timing, orientation/filter controls and HDMI PHY are not modeled"]}
        if backdrop_used:
            state["limitations"].append("Black behind transparent graphics is a declared canvas fixture, not a captured camera background")
        return self._finish_frame(ScanoutFrame(raw, rgb, None, state), instance)

    def _main_frame(self, instance, cpu_id, pixels=True):
        registers = self.regs[instance]
        if not registers.get(0, 0) & 1:
            return None
        missing = self.MAIN_REQUIRED - self.written[instance]
        if missing:
            raise UnsupportedScanout("Missing main-video registers: " + ", ".join(hex(n) for n in sorted(missing)))
        dimensions = registers[0x28]
        width, height = ((dimensions >> 16) & 0x1fff) + 1, (dimensions & 0x1fff) + 1
        canvas = registers[0x10]
        canvas_width, canvas_height = ((canvas >> 16) & 0xfff) + 1, (canvas & 0xfff) + 1
        if width * height > self.max_pixels:
            raise UnsupportedScanout("Display pixel budget exceeded")
        if (canvas_width, canvas_height) != (width, height):
            raise UnsupportedScanout("Video source/canvas geometry differs; scaling or placement requires further modeling")
        if registers[0x24] or registers[0xd8] or registers[0xdc] or registers[0xd4] != 0x10001000:
            raise UnsupportedScanout("Only zero-origin, zero-phase, 1:1 Q12 video scanout is supported")
        source_word, stride_word = registers[0x2c], registers[0x30]
        if source_word & 0xc0000000 or stride_word & ~0xfff or dimensions & 0xe000e000:
            raise UnsupportedScanout("Unresolved video source/format/stride control bits")
        source, padding = source_word & 0x3fffffff, stride_word & 0xfff
        stride = (width + padding) * 2
        count = (height - 1) * stride + width * 2
        raw = self._read_ram(source, count, cpu_id)
        rgb = png = None
        if pixels:
            try:
                rgb = render_rgb(raw, width, height, stride=stride, packing=self.packing, matrix=self.matrix)
            except ValueError as exc:
                raise UnsupportedScanout(str(exc)) from exc
        state = {"instance": instance, "image_base": hex(self.BASES[instance]),
                 "width": width, "height": height, "source": hex(source),
                 "source_cpu_alias": hex(source | 0x40000000), "stride": stride,
                 "padding_pixels": padding, "packing": self.packing, "matrix": self.matrix,
                 "frame_sha256": hashlib.sha256(raw).hexdigest(),
                 "png_sha256": None,
                 "kind": "main video plane at explicit frame boundary",
                 "limitations": ["Packing/matrix is an explicit interpretation; hardware validation remains pending",
                                 "Panel timing, filter coefficients and HDMI PHY are not modeled"]}
        return ScanoutFrame(raw, rgb, png, state)

    def _rectangles(self, instance):
        registers = self.regs[instance]
        enabled = registers.get(0x108, 0) & 0x303ff
        if not enabled:
            return ()
        if self.rectangle_profile is not RectangleProfile.XM1:
            raise UnsupportedScanout("Enabled rectangle/color overlays need an explicit interpretation")
        if enabled & 0x30000:
            raise UnsupportedScanout("X-M1 auxiliary color-region rasterization is unresolved")
        rectangles = []
        # X-M1 006CA04C, 006CA328 and 006CA3E0 pack these fields.
        for index in range(10):
            if not enabled & (1 << index):
                continue
            offsets = {0x11c + index * 8, 0x120 + index * 8, 0x174, 0x188, 0x190, 0x18c}
            color_slot = registers.get(0x174, 0) >> (index * 2) & 3
            width_slot = registers.get(0x188, 0) >> (index * 2) & 3
            color_offset = 0x178 + color_slot * 4
            width_offset = 0x16c + width_slot // 2 * 4
            offsets.update((color_offset, width_offset))
            if not offsets.issubset(self.written[instance]):
                raise UnsupportedScanout("Missing X-M1 rectangle geometry or style registers")
            start, end = (registers[0x11c + index * 8], registers[0x120 + index * 8])
            if (start | end) & 0xf000f000:
                raise UnsupportedScanout("Unresolved X-M1 rectangle coordinate controls")
            bounds = RectangleBounds(Pixel(start >> 16), Pixel(start & 0xfff),
                                     Pixel(end >> 16), Pixel(end & 0xfff))
            if bounds.left > bounds.right or bounds.top > bounds.bottom:
                raise UnsupportedScanout("Reversed X-M1 rectangle endpoints")
            thickness_word = registers[width_offset] >> (width_slot % 2 * 16) & 0xffff
            color = registers[color_offset]
            alpha_slot = registers[0x190] >> index & 1
            opacity = registers[0x18c] >> (alpha_slot * 8) & 0xff
            if color > 255 or thickness_word & ~0x7ff or opacity > 128:
                raise UnsupportedScanout("Unresolved X-M1 rectangle color, width or alpha controls")
            if thickness_word == 0 and opacity != 0:
                raise UnsupportedScanout("Opaque X-M1 rectangle has unresolved zero stroke width")
            rectangles.append(RectangleOverlay(RectangleIndex(index), bounds,
                                               Pixel(thickness_word), Luma(color), Opacity(opacity)))
        return tuple(rectangles)

    def _finish_frame(self, frame, instance):
        rectangles = self._rectangles(instance)
        if frame is None:
            if rectangles:
                raise UnsupportedScanout("X-M1 rectangles have no initialized display plane")
            return None
        if rectangles:
            frame.state["rectangle_profile"] = self.rectangle_profile.value
            frame.state["rectangles"] = [asdict(rectangle) for rectangle in rectangles]
            digest = hashlib.sha256(frame.raw)
            for rectangle in rectangles:
                bounds = rectangle.bounds
                digest.update(struct.pack("<8I", rectangle.index, bounds.left, bounds.top,
                                          bounds.right, bounds.bottom, rectangle.thickness,
                                          rectangle.luma, rectangle.opacity))
                if frame.rgb is None or not rectangle.opacity:
                    continue
                height, width, _ = frame.rgb.shape
                left, top, right, bottom = bounds.left, bounds.top, bounds.right + 1, bounds.bottom + 1
                inner_top = min(top + rectangle.thickness, bottom)
                inner_bottom = max(inner_top, bottom - rectangle.thickness)
                inner_left = min(left + rectangle.thickness, right)
                inner_right = max(inner_left, right - rectangle.thickness)
                for x0, y0, x1, y1 in ((left, top, right, inner_top),
                                       (left, inner_bottom, right, bottom),
                                       (left, inner_top, inner_left, inner_bottom),
                                       (inner_right, inner_top, right, inner_bottom)):
                    region = frame.rgb[min(y0, height):min(y1, height), min(x0, width):min(x1, width)]
                    if rectangle.opacity == 128:
                        region[:] = rectangle.luma
                    else:
                        region[:] = ((region.astype(np.uint32) * (128 - rectangle.opacity)
                                      + rectangle.luma * rectangle.opacity + 64) // 128).astype(np.uint8)
            frame.state["frame_sha256"] = digest.hexdigest()
            frame.state["limitations"].append("Rectangle grayscale, inclusive inner strokes, canvas clipping and topmost order are declared, unmeasured interpretations")
        frame.png = encode_png(frame.rgb) if frame.rgb is not None else None
        frame.state["png_sha256"] = hashlib.sha256(frame.png).hexdigest() if frame.png is not None else None
        return frame

    def snapshot(self):
        return {"device": type(self).__name__, "packing": self.packing, "matrix": self.matrix,
                "graphics_profile": self.graphics_profile,
                "rectangle_profile": None if self.rectangle_profile is None else self.rectangle_profile.value,
                "backdrop_profile": self.backdrop_profile,
                "instances": [{"instance": instance, "image_base": hex(base),
                    "registers": {hex(base + offset): hex(value) for offset, value in sorted(self.regs[instance].items())},
                    "written_offsets": [hex(n) for n in sorted(self.written[instance])],
                    "frames": self.frames[instance], "reset_completions": self.reset_completions[instance],
                    "last_frame": self.frame_state[instance], "render_error": self.render_errors[instance],
                    "graphics": self._graphics(instance)} for instance, base in enumerate(self.BASES)],
                "limits": ["Main video plane supports1:1 scaling only",
                           "Graphics mode1 needs an explicit BGRA interpretation profile; other graphics/solid modes fail rendering",
                           "Optional black canvas applies only behind enabled graphics when main video is absent; its color is unmeasured",
                           "No timing/IRQ generation; frame boundaries are explicit caller events",
                           "Zero initial image registers are a fixture; physical reset values are not captured"]}
