"""Tests for the `graph_collapse` binding."""

import pixelator_core
import pyarrow as pa
import pytest


def _umi(seq):
    return pixelator_core.encode_dna_2bit(seq)


def _reader(rows):
    """A record-batch reader with one batch per group of rows.

    Each group is `(umi1, umi2, component)` tuples, with an integer component
    id. Groups become separate batches so a component can cross a batch boundary.
    """
    schema = pa.schema(
        [
            ("umi1", pa.uint64()),
            ("umi2", pa.uint64()),
            ("component", pa.uint32()),
        ]
    )
    batches = [
        pa.record_batch(
            {
                "umi1": pa.array([row[0] for row in group], type=pa.uint64()),
                "umi2": pa.array([row[1] for row in group], type=pa.uint64()),
                "component": pa.array([row[2] for row in group], type=pa.uint32()),
            },
            schema=schema,
        )
        for group in rows
    ]
    return pa.RecordBatchReader.from_batches(schema, batches)


def _pairs(table):
    return set(
        zip(
            table.column("original").to_pylist(),
            table.column("corrected").to_pylist(),
        )
    )


def test_chain_collapses_within_its_component_only():
    """A leaf chain split across batches collapses onto its end, and a second component does not.

    CCAA is one symbol from CAAA, and CAAA is one symbol from AAAA, the end of
    the chain. CAAA is also one symbol from CCAA; AAAA comes first because rows
    are searched in UMI order. GGGG and TTTT are umi1, and the other UMIs are
    umi2, so no UMI sits on both sides. The other component is a single edge
    between distant UMIs.
    """
    umi_m = _umi("GGGG")
    umi_h = _umi("AAAA")
    umi_a = _umi("CCAA")
    umi_b = _umi("CAAA")
    umi_n = _umi("TTTT")
    reader = _reader(
        [
            [(umi_m, umi_h, 0), (umi_m, umi_a, 0)],
            [
                (umi_m, umi_b, 0),
                (umi_n, umi_h, 0),
                (_umi("ACAC"), _umi("GTGT"), 1),
            ],
        ]
    )

    table, _stats = pixelator_core.graph_collapse(
        reader, n_threads=1, bfs_threshold=2, hamming_threshold=1
    )

    assert table.column_names == ["component", "original", "corrected"]
    assert _pairs(table) == {(umi_a, umi_h), (umi_b, umi_h)}


def test_stats_count_what_the_search_did():
    """The stats dict covers every component, with the keys the report will use.

    This is the chain from the test above, plus the second component. Component 0
    has three leaves (CCAA, CAAA and TTTT) and component 1 has two. Only CCAA -> CAAA and
    CAAA -> AAAA are found, each two hops and one symbol apart.
    """
    umi_m = _umi("GGGG")
    umi_h = _umi("AAAA")
    umi_a = _umi("CCAA")
    umi_b = _umi("CAAA")
    umi_n = _umi("TTTT")
    reader = _reader(
        [
            [(umi_m, umi_h, 0), (umi_m, umi_a, 0)],
            [
                (umi_m, umi_b, 0),
                (umi_n, umi_h, 0),
                (_umi("ACAC"), _umi("GTGT"), 1),
            ],
        ]
    )

    _table, stats = pixelator_core.graph_collapse(
        reader, n_threads=1, bfs_threshold=2, hamming_threshold=1
    )

    assert stats == {
        "total_components": 2,
        "nodes_considered": 5,
        "targets_found": 2,
        "bfs_distance_distribution": {2: 2},
        "hamming_distance_distribution": {1: 2},
    }


def test_stats_distributions_are_in_ascending_distance_order():
    """Dict insertion order carries the ordering, so it must be ascending.

    Component 0 is the path AAAA - TTTT - GGGG - CGCG - CAAA - GCGC, where the umi1
    ends AAAA and CAAA are four hops and one symbol apart, and AAAA is the only
    leaf with a match. Component 1 is a star around GGGG with leaves AAAA and CCAA,
    two hops and two symbols apart, of which one match is kept. Component 0 comes
    first, so a tally in arrival order would list distance 4 before 2.
    """
    reader = _reader(
        [
            [
                (_umi("AAAA"), _umi("TTTT"), 0),
                (_umi("GGGG"), _umi("TTTT"), 0),
                (_umi("GGGG"), _umi("CGCG"), 0),
                (_umi("CAAA"), _umi("CGCG"), 0),
                (_umi("CAAA"), _umi("GCGC"), 0),
            ],
            [
                (_umi("GGGG"), _umi("AAAA"), 1),
                (_umi("GGGG"), _umi("CCAA"), 1),
            ],
        ]
    )

    _table, stats = pixelator_core.graph_collapse(
        reader, n_threads=1, bfs_threshold=4, hamming_threshold=2
    )

    assert stats["total_components"] == 2
    assert stats["nodes_considered"] == 4
    assert stats["targets_found"] == 2
    assert list(stats["bfs_distance_distribution"].items()) == [(2, 1), (4, 1)]
    assert list(stats["hamming_distance_distribution"].items()) == [(1, 1), (2, 1)]


def test_max_degree_skips_nodes_above_the_cutoff():
    """A degree-2 node is searched only when max_degree is at least 2.

    AAAA has two neighbors and is one symbol from CAAA, which has three. Every
    other node is four symbols from the candidates on its own side, so the
    default (leaves only) collapses nothing.
    """
    umi_d = _umi("AAAA")
    umi_s = _umi("CAAA")
    rows = [
        [
            (umi_d, _umi("GGGG"), 0),
            (umi_d, _umi("TTTT"), 0),
            (umi_s, _umi("GGGG"), 0),
            (umi_s, _umi("CCCC"), 0),
            (umi_s, _umi("ATAT"), 0),
        ]
    ]

    leaves_only, leaves_only_stats = pixelator_core.graph_collapse(
        _reader(rows), n_threads=1, bfs_threshold=2, hamming_threshold=1
    )
    with_degree_two, with_degree_two_stats = pixelator_core.graph_collapse(
        _reader(rows), n_threads=1, bfs_threshold=2, hamming_threshold=1, max_degree=2
    )

    assert _pairs(leaves_only) == set()
    assert _pairs(with_degree_two) == {(umi_d, umi_s)}
    # The three leaves are searched either way. Degree 2 adds AAAA and GGGG.
    assert leaves_only_stats["nodes_considered"] == 3
    assert leaves_only_stats["targets_found"] == 0
    assert with_degree_two_stats["nodes_considered"] == 5
    assert with_degree_two_stats["targets_found"] == 1


def test_row_order_within_a_component_does_not_change_the_result():
    """A leaf with two equally good targets gets the same one for any row order.

    AAAA is a leaf of GGGG. CAAA and GAAA are also neighbors of GGGG, each one
    symbol from AAAA, and each has a second neighbor, so neither is a leaf. Which
    one the search meets first depends on the neighbor order of GGGG, which would
    otherwise follow the row order.
    """
    rows = [
        (_umi("GGGG"), _umi("AAAA"), 0),
        (_umi("GGGG"), _umi("CAAA"), 0),
        (_umi("GGGG"), _umi("GAAA"), 0),
        (_umi("TTTT"), _umi("CAAA"), 0),
        (_umi("ACAC"), _umi("GAAA"), 0),
    ]
    orders = [rows, rows[::-1], [rows[i] for i in (2, 0, 4, 1, 3)]]

    results = [
        _pairs(
            pixelator_core.graph_collapse(
                _reader([order]), n_threads=1, bfs_threshold=2, hamming_threshold=1
            )[0]
        )
        for order in orders
    ]

    assert len(results[0]) == 1
    assert results[1] == results[0]
    assert results[2] == results[0]


def test_empty_edgelist_returns_an_empty_table():
    """No rows still produce the original/corrected schema and all-zero stats."""
    reader = _reader([])

    table, stats = pixelator_core.graph_collapse(reader, n_threads=1)

    assert table.num_rows == 0
    assert table.column_names == ["component", "original", "corrected"]
    assert stats == {
        "total_components": 0,
        "nodes_considered": 0,
        "targets_found": 0,
        "bfs_distance_distribution": {},
        "hamming_distance_distribution": {},
    }


def test_a_component_column_of_another_type_is_a_value_error():
    schema = pa.schema(
        [("umi1", pa.uint64()), ("umi2", pa.uint64()), ("component", pa.string())]
    )
    batch = pa.record_batch(
        {
            "umi1": pa.array([1], type=pa.uint64()),
            "umi2": pa.array([2], type=pa.uint64()),
            "component": pa.array(["a"]),
        },
        schema=schema,
    )
    reader = pa.RecordBatchReader.from_batches(schema, [batch])

    with pytest.raises(ValueError, match="Column component must be UInt32, got Utf8"):
        pixelator_core.graph_collapse(reader, n_threads=1)
