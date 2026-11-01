"""Validate the selected executable, never infer its width from its filename.

No ledger changes or work scheduling should happen before these checks.
Cache only against file identity/mtime/size, so a rebuild in this process is
not mistaken for the previously probed binary. Read-only ledger reports need
no executable and deliberately do not call this module.
"""

from functools import lru_cache
import json
from pathlib import Path
import shutil
import subprocess


def _identity(path):
    stat = path.stat()
    return stat.st_dev, stat.st_ino, stat.st_size, stat.st_mtime_ns, stat.st_ctime_ns


def validate(data):
    if not isinstance(data, dict) or data.get("schema") != "graphy-capabilities-v1":
        raise ValueError("unsupported graphy capability schema; rebuild the binary/launcher together")
    for name in ("bit_width", "max_order", "max_edges", "copy_count_bits", "orbital_enum_max_variables"):
        if type(data.get(name)) is not int or data[name] < 1:
            raise ValueError(f"invalid graphy capability: {name}")
    for name in ("sat", "nauty", "native"):
        if type(data.get(name)) is not bool:
            raise ValueError(f"invalid graphy capability: {name}")
    n, width = data["max_order"], data["bit_width"]
    if width not in (64, 128, 256) or data["max_edges"] != n * (n - 1) // 2 or not (
            data["max_edges"] <= width < n * (n + 1) // 2):
        raise ValueError("inconsistent graphy width/order capabilities")
    if data["copy_count_bits"] != 128:
        raise ValueError("this launcher requires the checked 128-bit copy counters")
    return data


@lru_cache(maxsize=16)
def _probe(path, identity):
    try:
        result = subprocess.run([path, "capabilities"], capture_output=True, text=True,
                                check=True, timeout=10)
    except (OSError, subprocess.SubprocessError) as error:
        raise ValueError(f"{path}: cannot query capabilities; rebuild this binary with the new code") from error
    if _identity(Path(path)) != identity:
        raise ValueError(f"{path}: executable changed during capability probe; retry")
    try:
        return validate(json.loads(result.stdout))
    except (ValueError, TypeError) as error:
        raise ValueError(f"{path}: invalid capabilities: {error}") from error


def check_orders(capabilities, orders):
    maximum = capabilities["max_order"]
    for n in orders:
        if type(n) is not int or not 1 <= n <= maximum:
            raise ValueError(f"order {n} exceeds this binary's supported range 1..{maximum} "
                             f"({capabilities['bit_width']}-bit); select --binary with sufficient width")


def require(binary, orders=(), *, sat=False):
    path = Path(shutil.which(binary) or binary).resolve()
    try:
        data = dict(_probe(str(path), _identity(path)))
    except OSError as error:
        raise ValueError(f"cannot read graphy binary {path}: {error}") from error
    check_orders(data, orders)
    if sat and not data["sat"]:
        raise ValueError(f"{path}: SAT support is absent; rebuild with --features sat (and u256 for wide graphs)")
    return data
