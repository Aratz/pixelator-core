use std::sync::Arc;

use arrow::array::{Array, AsArray, PrimitiveArray, RecordBatch, StringArray, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema, UInt64Type};
use arrow::error::ArrowError;
use arrow::pyarrow::{PyArrowType, Table};
use itertools::structs::ChunkBy;
use itertools::Itertools;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rayon::slice::ParallelSliceMut;
use rayon::ThreadPoolBuilder;

use pixelator_core::common::graph::Graph;
use pixelator_core::common::node_indexing::UmiToNodeIndexMapping;
use pixelator_core::common::types::UMI;

use pixelator_core::graph_collapse::{graph_collapse, CollapseStats};

fn to_py_err(err: impl std::fmt::Debug) -> PyErr {
    PyRuntimeError::new_err(format!("{err:?}"))
}

/// One edgelist row: `(umi1, umi2, component)`.
type EdgelistRow = (u64, u64, String);

/// Collapse likely sequencing-error UMIs in an edgelist.
///
/// The edgelist must be sorted by `component`, so each component is a
/// contiguous run of rows. A component may span several record batches. The
/// rows within a component may come in any order: they are sorted before the
/// graph is built, so the result does not depend on it.
///
/// Args:
///     edgelist_reader: A pyarrow `RecordBatchReader` with `umi1` and `umi2`
///         (uint64) and `component` (string) columns, sorted by `component`.
///     n_threads: Number of worker threads for the per-component search.
///     bfs_threshold: Maximum graph distance searched for a correction target.
///     hamming_threshold: Maximum Hamming distance between an investigated UMI
///         and its correction target.
///     max_degree: Nodes with degree above this value are not searched. The
///         default, 1, investigates only leaves.
///
/// Returns:
///     A `(table, stats)` tuple.
///
///     `table` is a pyarrow `Table` with `original` and `corrected` uint64
///     columns, one row per collapsed UMI.
///
///     `stats` is a dict of what the search counted, over all components:
///
///     - `total_components`: components processed.
///     - `nodes_considered`: nodes with degree at most `max_degree`.
///     - `targets_found`: kept matches. A match dropped because it would close
///       a loop is not counted, and neither is a pair that the caller later
///       discards (for example for differing markers).
///     - `bfs_distance_distribution`: `{hops: count}` over kept matches.
///     - `hamming_distance_distribution`: `{distance: count}` over kept matches.
///
///     Both distributions list only distances that occurred, in ascending
///     order. Each distance is that of the match as found: for a chain
///     `A -> B -> H` it is the distance from A to B and from B to H, not from
///     A to H.
#[pyfunction(
    name = "graph_collapse",
    signature = (
        edgelist_reader,
        n_threads,
        bfs_threshold=6,
        hamming_threshold=2,
        max_degree=1,
    )
)]
pub fn py_graph_collapse<'py>(
    py: Python<'py>,
    edgelist_reader: PyArrowType<arrow::ffi_stream::ArrowArrayStreamReader>,
    n_threads: usize,
    bfs_threshold: u64,
    hamming_threshold: u8,
    max_degree: u64,
) -> PyResult<(PyArrowType<Table>, Bound<'py, PyDict>)> {
    // `graph_collapse` parallelizes on the current Rayon pool. A local pool
    // keeps that at `n_threads` instead of the CPU count. The GIL is released
    // for the duration: the pool thread reads the Arrow stream, whose
    // callbacks take the GIL themselves, and would deadlock if we still held it.
    let pool = ThreadPoolBuilder::new()
        .num_threads(n_threads)
        .build()
        .map_err(to_py_err)?;
    let PyArrowType(edgelist_reader) = edgelist_reader;
    let (table, stats) = py
        .detach(move || {
            pool.install(|| {
                run_graph_collapse(
                    edgelist_reader,
                    bfs_threshold,
                    hamming_threshold,
                    max_degree,
                )
            })
        })
        .map_err(to_py_err)?;

    Ok((PyArrowType(table), stats_to_dict(py, &stats)?))
}

/// Run graph collapse on every component of the edgelist.
///
/// Each component is loaded into its own graph and collapsed independently.
/// Reading stops at the first batch that fails, and that error is returned.
///
/// Returns the collapsed pairs together with the stats summed over components.
fn run_graph_collapse(
    edgelist_reader: impl Iterator<Item = Result<RecordBatch, ArrowError>>,
    bfs_threshold: u64,
    hamming_threshold: u8,
    max_degree: u64,
) -> Result<(Table, CollapseStats), ArrowError> {
    let mut global_stats = CollapseStats::default();
    let (original, corrected): (Vec<u64>, Vec<u64>) =
        edgelist_reader.process_results(|batches| {
            batches_to_graph_components(batches)
                .into_iter()
                .flat_map(|(_, component_edges)| {
                    let mut umi_pairs: Vec<_> = component_edges
                        .map(|(umi1, umi2, _)| (umi1 as UMI, umi2 as UMI))
                        .collect();
                    // Node indices and neighbor order follow the edge order, and they
                    // decide which match the search finds first. Sorting makes the result
                    // independent of the row order within a component.
                    umi_pairs.par_sort_unstable();

                    let umi_mapping =
                        UmiToNodeIndexMapping::from_umi_pairs(umi_pairs.iter().copied());
                    let num_nodes = umi_mapping.get_num_of_nodes();
                    let edges = umi_mapping.map_umi_pair_iterator_to_edge(umi_pairs);
                    let graph = Graph::<u8>::from_edges(edges, num_nodes);

                    let (targets, local_stats) = graph_collapse(
                        &graph,
                        &umi_mapping,
                        bfs_threshold as usize,
                        hamming_threshold,
                        max_degree as usize,
                    );
                    global_stats.add_components(1);
                    global_stats += local_stats;
                    targets
                })
                .map(|(original, corrected)| (original as u64, corrected as u64))
                .unzip()
        })?;

    arrow_table(original, corrected).map(|table| (table, global_stats))
}

/// Flatten the batches into `(umi1, umi2, component)` rows grouped by component.
///
/// Consecutive rows with the same `component` form one group, regardless of
/// batch boundaries. The groups must be consumed in order, so the result is
/// iterated with `(&result).into_iter()` or `for (component, rows) in &result`.
///
/// Panics if a column is missing, has the wrong type, or contains a null.
fn batches_to_graph_components(
    batch_iter: impl Iterator<Item = RecordBatch>,
) -> ChunkBy<String, impl Iterator<Item = EdgelistRow>, impl FnMut(&EdgelistRow) -> String> {
    batch_iter
        .flat_map(|batch| {
            let umi1 = uint64_column(&batch, "umi1");
            let umi2 = uint64_column(&batch, "umi2");
            let component = utf8_column(&batch, "component");
            let rows = umi1.len();
            (0..rows).map(move |row| {
                (
                    umi1.value(row),
                    umi2.value(row),
                    component.value(row).to_string(),
                )
            })
        })
        .chunk_by(|(_, _, component)| component.clone())
}

/// Get a non-null UInt64 column by name.
///
/// Panics if the column is missing, is not UInt64, or contains a null.
fn uint64_column(batch: &RecordBatch, name: &str) -> PrimitiveArray<UInt64Type> {
    let column = batch
        .column_by_name(name)
        .unwrap_or_else(|| panic!("Could not find {name} column in data"))
        .as_primitive_opt::<UInt64Type>()
        .unwrap_or_else(|| panic!("Column {name} is not UInt64"));
    assert_eq!(column.null_count(), 0, "{name} contains a null");
    column.clone()
}

/// Get a non-null Utf8 column by name.
///
/// Panics if the column is missing, is not Utf8, or contains a null.
fn utf8_column(batch: &RecordBatch, name: &str) -> StringArray {
    let column = batch
        .column_by_name(name)
        .unwrap_or_else(|| panic!("Could not find {name} column in data"))
        .as_string_opt::<i32>()
        .unwrap_or_else(|| panic!("Column {name} is not Utf8"));
    assert_eq!(column.null_count(), 0, "{name} contains a null");
    column.clone()
}

/// Build a single-batch table with non-null `original` and `corrected` UInt64 columns.
fn arrow_table(original: Vec<u64>, corrected: Vec<u64>) -> Result<Table, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("original", DataType::UInt64, false),
        Field::new("corrected", DataType::UInt64, false),
    ]));
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(UInt64Array::from(original)),
            Arc::new(UInt64Array::from(corrected)),
        ],
    )?;
    Table::try_new(vec![batch], schema)
}

/// Mirrors [`CollapseStats`] as a plain dict, with the keys listed in the
/// docstring of [`py_graph_collapse`].
fn stats_to_dict<'py>(py: Python<'py>, stats: &CollapseStats) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("total_components", stats.total_components)?;
    out.set_item("nodes_considered", stats.nodes_considered)?;
    out.set_item("targets_found", stats.targets_found)?;

    let bfs_distance_dict = PyDict::new(py);
    for (d, c) in stats.bfs_distance_counts() {
        bfs_distance_dict.set_item(d, c)?;
    }
    out.set_item("bfs_distance_distribution", bfs_distance_dict)?;

    let hamming_distance_dict = PyDict::new(py);
    for (d, c) in stats.hamming_distance_counts() {
        hamming_distance_dict.set_item(d, c)?;
    }
    out.set_item("hamming_distance_distribution", hamming_distance_dict)?;

    Ok(out)
}

#[cfg(test)]
mod tests {
    use pixelator_core::encoding::encode_dna_2bit;
    use rustc_hash::FxHashSet as HashSet;

    use super::*;

    fn umi(seq: &str) -> u64 {
        encode_dna_2bit(seq.as_bytes())
    }

    fn batch(umi1: &[u64], umi2: &[u64], component: &[&str]) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("umi1", DataType::UInt64, false),
            Field::new("umi2", DataType::UInt64, false),
            Field::new("component", DataType::Utf8, false),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(umi1.to_vec())),
                Arc::new(UInt64Array::from(umi2.to_vec())),
                Arc::new(StringArray::from(component.to_vec())),
            ],
        )
        .unwrap()
    }

    fn component_groups(batches: impl Iterator<Item = RecordBatch>) -> Vec<Vec<(u64, u64)>> {
        let grouped = batches_to_graph_components(batches);
        (&grouped)
            .into_iter()
            .map(|(_, rows)| rows.map(|(umi1, umi2, _)| (umi1, umi2)).collect())
            .collect()
    }

    fn collapsed_pairs(table: &Table) -> HashSet<(u64, u64)> {
        let batch = &table.record_batches()[0];
        let original = batch.column(0).as_primitive::<UInt64Type>().values();
        let corrected = batch.column(1).as_primitive::<UInt64Type>().values();
        original
            .iter()
            .copied()
            .zip(corrected.iter().copied())
            .collect()
    }

    #[test]
    fn component_runs_cross_batch_boundaries() {
        // [A, A, B] | [B, C] | [C]. B and C each continue into the next batch.
        let batches = [
            batch(&[1, 2, 3], &[10, 20, 30], &["A", "A", "B"]),
            batch(&[4, 5], &[40, 50], &["B", "C"]),
            batch(&[6], &[60], &["C"]),
        ];

        assert_eq!(
            component_groups(batches.into_iter()),
            vec![
                vec![(1, 10), (2, 20)],
                vec![(3, 30), (4, 40)],
                vec![(5, 50), (6, 60)],
            ]
        );
    }

    #[test]
    fn empty_batch_does_not_split_a_component() {
        let batches = [
            batch(&[1], &[10], &["A"]),
            batch(&[], &[], &[]),
            batch(&[2], &[20], &["A"]),
        ];

        assert_eq!(
            component_groups(batches.into_iter()),
            vec![vec![(1, 10), (2, 20)]]
        );
    }

    #[test]
    fn chain_collapses_within_its_component_only() {
        // A chain A -> B -> H, split so the component boundary falls in the
        // second batch. CCAA is one symbol from CAAA, and CAAA is one symbol from
        // both CCAA and AAAA. Rows are sorted before the search, so CAAA meets
        // AAAA, the chain's end, first. The other component is one edge between
        // two distant UMIs, which has no collapse target. M and N are umi1 and
        // every other UMI is umi2, so none sits on both sides.
        let umi_m = umi("GGGG");
        let umi_h = umi("AAAA");
        let umi_a = umi("CCAA");
        let umi_b = umi("CAAA");
        let umi_n = umi("TTTT");
        let batches = [
            batch(&[umi_m, umi_m], &[umi_h, umi_a], &["cell", "cell"]),
            batch(
                &[umi_m, umi_n, umi("ACAC")],
                &[umi_b, umi_h, umi("GTGT")],
                &["cell", "cell", "other"],
            ),
        ];

        let (table, stats) = run_graph_collapse(batches.into_iter().map(Ok), 2, 1, 1).unwrap();

        assert_eq!(
            collapsed_pairs(&table),
            HashSet::from_iter([(umi_a, umi_h), (umi_b, umi_h)])
        );
        // "cell" has three leaves (A, B and N) and "other" has two. Only the
        // chain in "cell" finds matches.
        assert_eq!(stats.total_components, 2);
        assert_eq!(stats.nodes_considered, 5);
        assert_eq!(stats.targets_found, 2);
        assert_eq!(stats.bfs_distance_counts(), [(2, 2)]);
        assert_eq!(stats.hamming_distance_counts(), [(1, 2)]);
    }

    #[test]
    fn empty_edgelist_returns_an_empty_table() {
        let (table, stats) = run_graph_collapse(std::iter::empty(), 2, 1, 1).unwrap();
        let batch = &table.record_batches()[0];

        assert_eq!(stats.total_components, 0);
        assert_eq!(stats.nodes_considered, 0);
        assert_eq!(stats.targets_found, 0);

        assert_eq!(batch.num_rows(), 0);
        assert_eq!(
            batch
                .schema()
                .fields()
                .iter()
                .map(|field| field.name().as_str())
                .collect::<Vec<_>>(),
            vec!["original", "corrected"]
        );
    }
}
