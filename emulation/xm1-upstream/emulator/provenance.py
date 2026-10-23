"""Separate the process's initial source snapshot from actual loaded models.

Import this helper near process startup. Source bytes are captured exactly
once, at this module's first import; later Board resets cannot refresh them.
Loaded Python functions and constants are fingerprinted from live module
objects at each call, never inferred from files currently on disk.
"""
from datetime import datetime, timezone
import hashlib
import importlib.metadata
import json
from pathlib import Path
import sys
from types import CodeType, FunctionType, ModuleType

_PACKAGE_ROOT = Path(__file__).resolve().parent
_PACKAGE_NAME = __package__


def _utc_now():
    return datetime.now(timezone.utc).isoformat()


def _json_digest(value):
    data = json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=True).encode("utf-8")
    return hashlib.sha256(data).hexdigest()


def _source_snapshot():
    files = []
    for path in sorted(_PACKAGE_ROOT.rglob("*.py")):
        if "__pycache__" in path.parts:
            continue
        name = path.relative_to(_PACKAGE_ROOT).as_posix()
        try:
            data = path.read_bytes()
        except OSError as exc:
            files.append({"path": name, "status": "unreadable", "error_type": type(exc).__name__})
        else:
            files.append({"path": name, "status": "captured", "bytes": len(data),
                          "sha256": hashlib.sha256(data).hexdigest()})
    return {"capture_point": "first import of the provenance helper in this process",
            "captured_at_utc": _utc_now(), "package": _PACKAGE_NAME,
            "files": files, "sha256": _json_digest(files),
            "limits": ["Not a verified OS-process-launch timestamp; import this helper near startup",
                       "Files are read sequentially once, not an atomic filesystem snapshot",
                       "Source bytes may differ from already loaded modules or cached bytecode",
                       "Later disk edits/additions/removals and Board/server resets do not refresh this snapshot"]}


# Keep the process snapshot as immutable serialized bytes, not an exposed
# mutable dictionary. Each public result reconstructs an independent copy.
_PROCESS_SOURCE_JSON = json.dumps(_source_snapshot(), sort_keys=True)
_PYTHON_RUNTIME = {"version": sys.version, "implementation": sys.implementation.name,
                   "cache_tag": sys.implementation.cache_tag}
try:
    _UNICORN_DISTRIBUTION_VERSION = importlib.metadata.version("unicorn")
except importlib.metadata.PackageNotFoundError:
    _UNICORN_DISTRIBUTION_VERSION = None


def _code_filename(filename):
    if filename.startswith("<"):
        return filename
    try:
        return "<model>/" + Path(filename).resolve().relative_to(_PACKAGE_ROOT).as_posix()
    except (ValueError, OSError):
        return filename


def _value(value, active=None):
    """Typed deterministic encoding, with explicit opaque/cyclic limits."""
    if active is None:
        active = set()
    if value is None:
        return {"type": "none"}
    if isinstance(value, bool):
        return {"type": "bool", "value": value}
    if isinstance(value, int):
        return {"type": "int", "value": str(value)}
    if isinstance(value, float):
        return {"type": "float", "hex": value.hex()}
    if isinstance(value, complex):
        return {"type": "complex", "real": value.real.hex(), "imag": value.imag.hex()}
    if isinstance(value, str):
        return {"type": "str", "value": value}
    if isinstance(value, bytes):
        return {"type": "bytes", "hex": value.hex()}
    if isinstance(value, Path):
        return {"type": "path", "value": str(value)}
    if isinstance(value, CodeType):
        return {"type": "code", "value": _code(value)}
    identifier = id(value)
    if identifier in active:
        return {"type": "cycle", "class": type(value).__module__ + "." + type(value).__qualname__}
    active.add(identifier)
    try:
        if isinstance(value, (tuple, list, set, frozenset)):
            items = [_value(item, active) for item in value]
            if isinstance(value, (set, frozenset)):
                items.sort(key=lambda item: json.dumps(item, sort_keys=True))
            return {"type": type(value).__name__, "items": items}
        if isinstance(value, dict):
            items = [[_value(key, active), _value(item, active)] for key, item in value.items()]
            items.sort(key=lambda item: json.dumps(item[0], sort_keys=True))
            return {"type": "dict", "items": items}
        return {"type": "opaque", "class": type(value).__module__ + "." + type(value).__qualname__}
    finally:
        active.remove(identifier)


def _code(code):
    # Explicit fields avoid pickles/marshal-version ambiguity and include
    # nested code objects (scheduled callbacks, comprehensions, closures).
    return {"name": code.co_name, "qualname": getattr(code, "co_qualname", code.co_name),
            "filename": _code_filename(code.co_filename), "first_line": code.co_firstlineno,
            "argcount": code.co_argcount, "posonlyargcount": code.co_posonlyargcount,
            "kwonlyargcount": code.co_kwonlyargcount, "nlocals": code.co_nlocals,
            "stacksize": code.co_stacksize, "flags": code.co_flags,
            "bytecode_hex": code.co_code.hex(), "constants": [_value(v) for v in code.co_consts],
            "names": list(code.co_names), "varnames": list(code.co_varnames),
            "freevars": list(code.co_freevars), "cellvars": list(code.co_cellvars),
            "line_table_hex": (code.co_linetable if hasattr(code, "co_linetable") else code.co_lnotab).hex(),
            "exception_table_hex": getattr(code, "co_exceptiontable", b"").hex()}


def _function(function):
    closure = []
    for name, cell in zip(function.__code__.co_freevars, function.__closure__ or ()):
        try:
            contents = cell.cell_contents
            if name == "__classdict__" and isinstance(contents, dict):
                # copyreg adds this derived slot-name cache during deepcopy.
                contents = {key: value for key, value in contents.items() if key != "__slotnames__"}
            closure.append(_value(contents))
        except ValueError:
            closure.append({"type": "empty_cell"})
    data = {"code": _code(function.__code__), "defaults": _value(function.__defaults__),
            "kwdefaults": _value(function.__kwdefaults__), "closure": closure}
    return {"sha256": _json_digest(data), "origin_module": function.__module__,
            "qualname": function.__qualname__, "filename": _code_filename(function.__code__.co_filename),
            "first_line": function.__code__.co_firstlineno}


def _class(cls):
    functions, constants = {}, {}
    for name, member in sorted(vars(cls).items()):
        # Python 3.14's per-class annotation evaluator closes over the live class dict.
        if name == "__annotate_func__":
            continue
        if isinstance(member, (staticmethod, classmethod)):
            functions[name] = {"descriptor": type(member).__name__, **_function(member.__func__)}
        elif isinstance(member, FunctionType):
            functions[name] = {"descriptor": "function", **_function(member)}
        elif isinstance(member, property):
            for label, function in (("get", member.fget), ("set", member.fset), ("delete", member.fdel)):
                if function is not None:
                    functions[name + "." + label] = {"descriptor": "property", **_function(function)}
        elif name.isupper():
            constants[name] = _value(member)
    data = {"bases": [base.__module__ + "." + base.__qualname__ for base in cls.__bases__],
            "functions": functions, "constants": constants}
    return {"sha256": _json_digest(data), **data}


# Pure parse memoization and lazy native-symbol resolution are derived from
# the functions/constants below and the separately pinned native binary.
# Their populated contents are process history, not model defining inputs.
_DERIVED_MODULE_GLOBALS = {
    "emulator.graphics_devices": frozenset(("_UI_STREAM_CACHE", "_NATIVE_STREAM",
        "_NATIVE_FILL", "_NATIVE_COMPOSITE", "_NATIVE_COMPOSITE_TILED")),
    "emulator.loader_devices": frozenset(("_NATIVE_FEED",)),
}


def _loaded_module(module):
    functions, classes, constants = {}, {}, {}
    derived = _DERIVED_MODULE_GLOBALS.get(module.__name__, ())
    # Read __dict__ directly, avoiding dynamic module __getattr__ and property
    # evaluation. Imported native/library classes are not expanded.
    for name, member in sorted(vars(module).items()):
        if name in derived:
            continue
        if isinstance(member, FunctionType):
            functions[name] = _function(member)
        elif isinstance(member, type) and member.__module__ == module.__name__:
            classes[name] = _class(member)
        elif name.isupper():
            constants[name] = _value(member)
    data = {"functions": functions, "classes": classes, "constants": constants}
    if derived:
        data["derived_runtime_globals"] = sorted(derived)
    return {"sha256": _json_digest(data), **data}


def model_provenance():
    """Return JSON-safe evidence for current loaded models and initial source.

    The loaded-code digest can change after an actual import/reload or constant
    mutation. Editing source files alone cannot change it. No modules reload
    or import as a side effect of this call.
    """
    modules = {}
    for name, module in sorted(tuple(sys.modules.items())):
        if (isinstance(module, ModuleType) and name != __name__ and
                (name == _PACKAGE_NAME or name.startswith(_PACKAGE_NAME + "."))):
            modules[name] = _loaded_module(module)
    unicorn = sys.modules.get("unicorn")
    loaded_unicorn_version = vars(unicorn).get("__version__") if isinstance(unicorn, ModuleType) else None
    return {"schema": "x70view-model-provenance-v1",
            "source_at_process_start": json.loads(_PROCESS_SOURCE_JSON),
            "loaded_python_models": {"captured_at_utc": _utc_now(), "modules": modules,
                "sha256": _json_digest({name: data["sha256"] for name, data in modules.items()}),
                "method": "Live function code objects/defaults/closures and module/class constants; no source reread",
                "limits": ["Only currently imported model-package modules are included; provenance helper excluded",
                           "Uppercase-named constants are included; lowercase globals, instance/device state and opaque objects are not",
                           "Explicit derived parse/native-symbol caches are excluded; their exclusion names are fingerprinted",
                           "Top-level module statements no longer have a retained code object; their constants/functions are represented",
                           "No native C/shared-library bytes, RTOS correctness or hardware equivalence are validated",
                           "Bytecode/code-position fingerprints are Python-version dependent",
                           "Modules/constants are inspected sequentially; concurrent mutation is not an atomic snapshot"]},
            "runtime": {"python": dict(_PYTHON_RUNTIME),
                "unicorn": {"loaded_binding_version": loaded_unicorn_version,
                    "distribution_version_at_helper_import": _UNICORN_DISTRIBUTION_VERSION,
                    "limits": "Version metadata is not a digest of the loaded native Unicorn library"}}}
