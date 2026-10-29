"""Extract divan benchmark metrics and output them to json.

Timing rows become seconds. `mem_*` benches also contribute peak-heap
(`max alloc` size, converted to MiB) from Divan's AllocProfiler.
"""

import json
import re
import sys

TIMING_UNITS = {
    "m": 60.0,
    "s": 1.0,
    "ms": 1.0e-3,
    "µs": 1.0e-6,
}

BYTE_UNITS = {
    "B": 1.0,
    "KB": 1e3,
    "MB": 1e6,
    "GB": 1e9,
    "TB": 1e12,
    "KiB": 1024.0,
    "MiB": 1024.0**2,
    "GiB": 1024.0**3,
    "TiB": 1024.0**4,
}

BENCH_LINE = re.compile(
    r"(\w+)\s+(\d+(?:\.\d+)?\s+\S+)\s*│\s*"
    r"(\d+(?:\.\d+)?\s+\S+)\s*│\s*"
    r"(\d+(?:\.\d+)?\s+\S+)\s*│\s*"
    r"(\d+(?:\.\d+)?\s+\S+)"
)
# A tree row names a node: `├─ name` or `╰─ name`, indented three characters
# per level. Rows with a timing column are benchmarks; the rest are headers.
TREE_NODE = re.compile(r"^([│ ]*[├╰]─ )(\w+)")
# Tree levels: root (0) > module (1) > benchmark function (2) > argument (3).
# Divan prints an `args = [...]` benchmark as a header row for the function
# with one timed row per argument, so those rows are named after the function.
# This assumes benchmark modules are not nested.
FUNCTION_DEPTH = 2
BYTE_VALUE = re.compile(
    r"(\d+(?:\.\d+)?)\s+(B|KiB|MiB|GiB|TiB|KB|MB|GB|TB)\b"
)


def to_seconds(cell: str) -> float:
    value, unit = cell.split()
    return float(value) * TIMING_UNITS[unit]


def to_mib(value: str, unit: str) -> float:
    return float(value) * BYTE_UNITS[unit] / (1024.0**2)


def parse(text: str) -> list[dict]:
    results = []
    lines = text.splitlines()
    current_name = None
    # Most recent node name at each tree depth.
    node_at_depth: dict[int, str] = {}

    for i, line in enumerate(lines):
        node = TREE_NODE.match(line)
        if node:
            depth = len(node.group(1)) // 3
            node_at_depth[depth] = node.group(2)

        match = BENCH_LINE.search(line)
        if match:
            current_name = match.group(1)
            if node and depth > FUNCTION_DEPTH:
                current_name = f"{node_at_depth[FUNCTION_DEPTH]}/{current_name}"
            if not current_name.startswith("mem_"):
                results.append(
                    {
                        "name": current_name,
                        "value": to_seconds(match.group(3)),
                        "unit": "s",
                    }
                )
            continue

        if current_name is None or not current_name.startswith("mem_"):
            continue
        if "max alloc:" not in line:
            continue

        for follow in lines[i + 1 : i + 6]:
            sizes = BYTE_VALUE.findall(follow)
            if not sizes:
                continue
            # Match the timing parser: second column is slowest.
            value, unit = sizes[1] if len(sizes) > 1 else sizes[0]
            results.append(
                {
                    "name": current_name,
                    "value": round(to_mib(value, unit), 4),
                    "unit": "MiB",
                }
            )
            current_name = None
            break

    return results


text = open(sys.argv[1]).read()
results = parse(text)
print(results)
if not results:
    sys.exit(1)
if not any(r["name"].startswith("mem_") for r in results):
    sys.exit("No mem_* peak-heap rows found in divan output")
json.dump(results, open(sys.argv[2], "w"))
