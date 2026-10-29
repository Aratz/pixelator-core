"""Inputs for the `run_edgelist` tests, built from the Rust crate's fixtures.

`run_edgelist` accepts any object shaped like pixelator's `PNAAssay` and
`PNAAntibodyPanel`, so these stand-ins let the tests run without installing
pixelator. The assay and panel are loaded from the same files the Rust unit tests
use, and the reads are generated from them, so the PIDs and UMIs the Rust side
looks up are the ones in the reads. Otherwise every read would be dropped.
"""

from __future__ import annotations

import csv
import gzip
import random
from dataclasses import dataclass, field
from pathlib import Path

import yaml

FIXTURES = (
    Path(__file__).resolve().parents[2] / "pixelator_core" / "src" / "test_fixtures"
)

# umi1(28) + pid1(10) + lbs1(28) + uei(15) + lbs2(14) + pid2(10) + umi2(28).
# A single-end read must span the whole amplicon.
AMPLICON_LENGTH = 133
UMI_LENGTH = 28
UEI_LENGTH = 15

_COMPLEMENT = str.maketrans("ACGT", "TGCA")


@dataclass
class Region:
    """Duck-typed `PNARegion`: the attributes the binding copies across."""

    region_id: str
    region_type: str
    name: str
    sequence_type: str
    sequence: str | None = None
    min_len: int | None = None
    max_len: int | None = None
    regions: list[Region] = field(default_factory=list)


@dataclass
class Assay:
    """Duck-typed `PNAAssay`."""

    name: str
    assay_spec: list[Region]

    def sequence_of(self, region_id: str) -> str:
        """The fixed sequence of the region called `region_id`."""
        stack = list(self.assay_spec)
        while stack:
            region = stack.pop()
            if region.region_id == region_id:
                assert region.sequence is not None
                return region.sequence
            stack.extend(region.regions)
        raise KeyError(region_id)


class _Table:
    def __init__(self, rows: list[dict[str, str]]):
        self._rows = rows

    def to_dicts(self) -> list[dict[str, str]]:
        return self._rows


@dataclass
class Panel:
    """Duck-typed `PNAAntibodyPanel`: the binding calls `to_polars().to_dicts()`."""

    rows: list[dict[str, str]]

    def to_polars(self) -> _Table:
        return _Table(self.rows)


def _region(raw: dict) -> Region:
    raw = dict(raw)
    raw["regions"] = [_region(sub) for sub in raw.get("regions") or []]
    return Region(**raw)


def load_assay() -> Assay:
    raw = yaml.safe_load((FIXTURES / "proxiome-v2.yaml").read_text())
    return Assay(raw["name"], [_region(region) for region in raw["assay_spec"]])


def load_panel() -> Panel:
    with open(FIXTURES / "proxiome-v2-immuno-155-v2.0.csv", newline="") as fh:
        # The file opens with a commented YAML header.
        rows = csv.DictReader(line for line in fh if not line.startswith("#"))
        return Panel(list(rows))


def reverse_complement(seq: str) -> str:
    return seq.translate(_COMPLEMENT)[::-1]


def _random_bases(rng: random.Random, n: int) -> str:
    return "".join(rng.choices("ACGT", k=n))


def amplicons(n: int, assay: Assay, panel: Panel, seed: int = 0) -> list[str]:
    """`n` valid amplicons: umi1, pid1, lbs1, uei, lbs2, pid2 and umi2 in order."""
    rng = random.Random(seed)
    lbs1, lbs2 = assay.sequence_of("lbs-1"), assay.sequence_of("lbs-2")
    out = []
    for _ in range(n):
        marker_1, marker_2 = rng.choice(panel.rows), rng.choice(panel.rows)
        out.append(
            _random_bases(rng, UMI_LENGTH)
            + marker_1["sequence_1"]
            + lbs1
            + _random_bases(rng, UEI_LENGTH)
            + lbs2
            + marker_2["sequence_2"]
            + _random_bases(rng, UMI_LENGTH)
        )
    assert all(len(amplicon) == AMPLICON_LENGTH for amplicon in out)
    return out


def with_n_in_umi(amplicon: str) -> str:
    """The same amplicon with an `N` in its first UMI, so it is dropped as `has_n`."""
    return "N" + amplicon[1:]


def _write_fastq(path: Path, sequences: list[str]) -> Path:
    with gzip.open(path, "wt") as fh:
        for i, seq in enumerate(sequences):
            fh.write(f"@read{i}\n{seq}\n+\n{'I' * len(seq)}\n")
    return path


def write_paired(
    directory: Path, amplicon_list: list[str], r1_len: int = 70, r2_len: int = 90
) -> tuple[Path, Path]:
    """Paired reads: R1 from the amplicon start, R2 reverse-complemented from its end."""
    r1 = _write_fastq(directory / "R1.fastq.gz", [a[:r1_len] for a in amplicon_list])
    r2 = _write_fastq(
        directory / "R2.fastq.gz",
        [reverse_complement(a[-r2_len:]) for a in amplicon_list],
    )
    return r1, r2


def write_single_forward(directory: Path, amplicon_list: list[str]) -> Path:
    return _write_fastq(directory / "forward.fastq.gz", amplicon_list)


def write_single_reverse(directory: Path, amplicon_list: list[str]) -> Path:
    return _write_fastq(
        directory / "reverse.fastq.gz",
        [reverse_complement(a) for a in amplicon_list],
    )
