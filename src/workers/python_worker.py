#!/usr/bin/env python3
"""
Juglans Python Worker

JSON-RPC based worker process that executes Python function calls
requested by the Juglans runtime via stdin/stdout communication.

Protocol:
  Request:  {"id": "req-001", "type": "call", "target": "pandas", "method": "read_csv", "args": [...], "kwargs": {...}}
  Response: {"id": "req-001", "type": "value", "value": ..., "ref": null, "error": null}

For non-JSON-serializable objects, we return a reference ID and keep the object in memory.
"""

import sys
import json
import importlib
import traceback
from typing import Any, Dict, Optional
from pathlib import Path

# Object reference store for non-serializable Python objects
_object_refs: Dict[str, Any] = {}
_ref_counter = 0


def _generate_ref_id() -> str:
    """Generate a unique reference ID for an object."""
    global _ref_counter
    _ref_counter += 1
    return f"ref:{_ref_counter:06d}"


def _is_json_serializable(obj: Any) -> bool:
    """Check if an object can be JSON serialized."""
    try:
        json.dumps(obj)
        return True
    except (TypeError, ValueError, OverflowError):
        return False


def _serialize_result(obj: Any) -> tuple[str, Any, Optional[str]]:
    """
    Serialize a result for JSON transport.
    Returns: (type, value, ref)
    - type: "value" | "ref" | "none"
    - value: JSON-serializable value or type description
    - ref: reference ID if object is stored
    """
    if obj is None:
        return ("none", None, None)

    if _is_json_serializable(obj):
        return ("value", obj, None)

    # Store non-serializable object and return a reference
    ref_id = _generate_ref_id()
    _object_refs[ref_id] = obj

    # Return type info for debugging
    type_name = type(obj).__name__
    module = type(obj).__module__
    full_type = f"{module}.{type_name}" if module != "builtins" else type_name

    return ("ref", {"__type__": full_type, "__repr__": repr(obj)[:200]}, ref_id)


def _resolve_target(target: str) -> Any:
    """
    Resolve a call target to a Python object.
    Target can be:
    - Module name: "pandas" -> import pandas
    - Reference ID: "ref:000001" -> _object_refs["ref:000001"]
    - File path: "./utils.py" -> import from file
    """
    if target.startswith("ref:"):
        if target not in _object_refs:
            raise ValueError(f"Unknown object reference: {target}")
        return _object_refs[target]

    # Handle file path imports
    if target.startswith("./") or target.startswith("../") or target.endswith(".py"):
        path = Path(target)
        if not path.exists():
            raise ImportError(f"Python file not found: {target}")
        spec = importlib.util.spec_from_file_location(path.stem, path)
        module = importlib.util.module_from_spec(spec)
        sys.modules[path.stem] = module
        spec.loader.exec_module(module)
        return module

    # Regular module import (handles nested like "sklearn.ensemble")
    return importlib.import_module(target)


def handle_call(request: Dict[str, Any]) -> Dict[str, Any]:
    """Handle a function/method call request."""
    req_id = request.get("id", "unknown")
    target = request.get("target", "")
    method = request.get("method", "")
    args = request.get("args", [])
    kwargs = request.get("kwargs", {})

    try:
        obj = _resolve_target(target)

        # Get the callable
        if method:
            # Method call on object/module
            if not hasattr(obj, method):
                raise AttributeError(f"'{target}' has no attribute '{method}'")
            func = getattr(obj, method)
        else:
            # Direct call on object (e.g., ref to a callable)
            func = obj

        # Execute the call
        if callable(func):
            result = func(*args, **kwargs)
        else:
            # Not callable, just return the attribute value
            result = func

        result_type, value, ref = _serialize_result(result)

        return {
            "id": req_id,
            "type": result_type,
            "value": value,
            "ref": ref,
            "error": None,
        }

    except Exception as e:
        return {
            "id": req_id,
            "type": "error",
            "value": None,
            "ref": None,
            "error": {
                "type": type(e).__name__,
                "message": str(e),
                "traceback": traceback.format_exc(),
            },
        }


def handle_getattr(request: Dict[str, Any]) -> Dict[str, Any]:
    """Handle attribute access on a reference."""
    req_id = request.get("id", "unknown")
    target = request.get("target", "")
    attr = request.get("attr", "")

    try:
        obj = _resolve_target(target)
        result = getattr(obj, attr)
        result_type, value, ref = _serialize_result(result)

        return {
            "id": req_id,
            "type": result_type,
            "value": value,
            "ref": ref,
            "error": None,
        }

    except Exception as e:
        return {
            "id": req_id,
            "type": "error",
            "value": None,
            "ref": None,
            "error": {
                "type": type(e).__name__,
                "message": str(e),
                "traceback": traceback.format_exc(),
            },
        }


def handle_del(request: Dict[str, Any]) -> Dict[str, Any]:
    """Handle reference deletion (garbage collection)."""
    req_id = request.get("id", "unknown")
    refs = request.get("refs", [])

    deleted = []
    for ref in refs:
        if ref in _object_refs:
            del _object_refs[ref]
            deleted.append(ref)

    return {
        "id": req_id,
        "type": "value",
        "value": {"deleted": deleted, "remaining": len(_object_refs)},
        "ref": None,
        "error": None,
    }


def handle_ping(request: Dict[str, Any]) -> Dict[str, Any]:
    """Handle ping request for health check."""
    return {
        "id": request.get("id", "unknown"),
        "type": "value",
        "value": {"status": "ok", "refs_count": len(_object_refs)},
        "ref": None,
        "error": None,
    }


def main():
    """Main loop: read JSON requests from stdin, write responses to stdout."""
    # Use line-buffered output for immediate response
    sys.stdout = open(sys.stdout.fileno(), mode='w', buffering=1)

    handlers = {
        "call": handle_call,
        "getattr": handle_getattr,
        "del": handle_del,
        "ping": handle_ping,
    }

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            request = json.loads(line)
        except json.JSONDecodeError as e:
            response = {
                "id": "unknown",
                "type": "error",
                "value": None,
                "ref": None,
                "error": {
                    "type": "JSONDecodeError",
                    "message": f"Invalid JSON: {e}",
                    "traceback": None,
                },
            }
            print(json.dumps(response), flush=True)
            continue

        req_type = request.get("type", "call")
        handler = handlers.get(req_type, handle_call)
        response = handler(request)
        print(json.dumps(response), flush=True)


if __name__ == "__main__":
    main()
