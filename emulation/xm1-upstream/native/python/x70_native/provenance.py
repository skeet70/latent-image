"""Native binary/toolchain and loaded Python adapter provenance."""
import hashlib
import json
from pathlib import Path
from emulator.provenance import _code
from . import _native
from .board import RustBoard, NativeCore, unicorn_library


def provenance():
    library = unicorn_library()
    return dict(backend="x70-native", version=_native.VERSION, rustc=_native.RUSTC_VERSION,
        target=_native.TARGET, pyo3=_native.PYO3_VERSION, python_abi="abi3-py310; GIL-enabled CPython",
        extension_sha256=hashlib.sha256(Path(_native.__file__).read_bytes()).hexdigest(),
        unicorn_abi=_native.UNICORN_ABI, unicorn_library_sha256=hashlib.sha256(library.read_bytes()).hexdigest(),
        loaded_adapters={f"{cls.__name__}.{name}": hashlib.sha256(json.dumps(
            _code(function.__code__),sort_keys=True,separators=(',',':')).encode()).hexdigest()
            for cls in (RustBoard, NativeCore) for name, function in vars(cls).items() if hasattr(function, "__code__")},
        loaded_adapter_encoding="Explicit Python code fields and typed constants; avoids marshal string-intern reference changes",
        limits=["Native engines and contexts are process-local and thread-affine.",
            "The currently mapped native library comes from the installed Unicorn wheel; no second engine build is bundled.",
            "Binary digests are sampled from disk; already mapped binaries and concurrent edits are not an atomic snapshot.",
            "Execution retains declared model assumptions; it does not validate physical camera behavior."])
