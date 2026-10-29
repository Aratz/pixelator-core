use std::sync::Arc;

use arrow::array::{Array, AsArray, RecordBatch, UInt32Array, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema, UInt32Type, UInt64Type};
use arrow::error::ArrowError;
use arrow::pyarrow::{PyArrowType, Table};

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

use pixelator_core::common::graph::Graph;
use pixelator_core::common::node_indexing::UmiToNodeIndexMapping;
use pixelator_core::common::types::UMI;
use pixelator_core::graph_collapse::{graph_collapse, CollapseStats};

fn to_py_err(err: impl std::fmt::Debug) -> PyErr {
    PyRuntimeError::new_err(format!("{err:?}"))
}

/// One edgelist row: `(component, umi1, umi2)`.
type EdgelistRow = (u32, u64, u64);

/// One collapsed UMI: `(component, original, corrected)`.
type Rename = (u32, u64, u64);

/// Collapse likely sequencing-error UMIs in an edgelist.
///
/// The rows are read into memory, so the caller bounds memory by passing one
/// batch of components at a time. Rows may come in any order, and a component
/// may span several record batches: rows are grouped by `component` and sorted
/// before each graph is built, so the result does not depend on the order.
/// Components are searched in parallel.
///
/// Args:
///     edgelist_reader: A pyarrow `RecordBatchReader` with `umi1` and `umi2`
///         (uint64) and `component` (uint32) columns. `component` only has to
///         tell components apart; the caller maps it back to a name.
///     n_threads: Number of worker threads for the search.
///     bfs_threshold: Maximum graph distance searched for a correction target.
///     hamming_threshold: Maximum Hamming distance between an investigated UMI
///         and its correction target.
///     max_degree: Nodes with degree above this value are not searched. The
///         default, 1, investigates only leaves.
///
/// Returns:
///     A `(table, stats)` tuple.
///
///     `table` is a pyarrow `Table` with a `component` uint32 column and
///     `original` and `corrected` uint64 columns, one row per collapsed UMI,
///     sorted by `component` and then `original`.
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
        .map_err(arrow_to_py_err)?;

    Ok((PyArrowType(table), stats_to_dict(py, &stats)?))
}

/// An input that does not match the expected columns is a `ValueError`; any
/// other failure, such as one raised while reading the stream, a `RuntimeError`.
fn arrow_to_py_err(err: ArrowError) -> PyErr {
    match err {
        ArrowError::SchemaError(message) => PyValueError::new_err(message),
        other => to_py_err(other),
    }
}

/// Run graph collapse on every component of the edgelist.
///
/// Each component is loaded into its own graph and collapsed independently,
/// in parallel. Reading stops at the first batch that fails, and that error is
/// returned.
///
/// Returns the collapsed UMIs together with the stats summed over components.
fn run_graph_collapse(
    edgelist_reader: impl Iterator<Item = Result<RecordBatch, ArrowError>>,
    bfs_threshold: u64,
    hamming_threshold: u8,
    max_degree: u64,
) -> Result<(Table, CollapseStats), ArrowError> {
    let mut rows = read_rows(edgelist_reader)?;
    // Node indices and neighbor order follow the edge order, and they decide
    // which match the search finds first. Sorting groups each component into
    // one run and makes the result independent of the row order.
    rows.par_sort_unstable();

    let per_component: Vec<(Vec<Rename>, CollapseStats)> = rows
        .chunk_by(|a, b| a.0 == b.0)
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|component_rows| {
            collapse_component(component_rows, bfs_threshold, hamming_threshold, max_degree)
        })
        .collect();

    let mut stats = CollapseStats::default();
    let mut renames = Vec::new();
    for (component_renames, component_stats) in per_component {
        stats.add_components(1);
        stats += component_stats;
        renames.extend(component_renames);
    }
    arrow_table(renames).map(|table| (table, stats))
}

/// Read every row of the stream into memory.
///
/// Fails with a schema error if a column is missing, has the wrong type, or
/// contains a null.
fn read_rows(
    edgelist_reader: impl Iterator<Item = Result<RecordBatch, ArrowError>>,
) -> Result<Vec<EdgelistRow>, ArrowError> {
    let mut rows = Vec::new();
    for batch in edgelist_reader {
        let batch = batch?;
        let component = uint32_column(&batch, "component")?;
        let umi1 = uint64_column(&batch, "umi1")?;
        let umi2 = uint64_column(&batch, "umi2")?;
        rows.extend(
            (0..batch.num_rows())
                .map(|row| (component.value(row), umi1.value(row), umi2.value(row))),
        );
    }
    Ok(rows)
}

/// Collapse one component, given its rows sorted by `(umi1, umi2)`.
///
/// Returns the component's renames, sorted by original UMI, and its stats.
fn collapse_component(
    rows: &[EdgelistRow],
    bfs_threshold: u64,
    hamming_threshold: u8,
    max_degree: u64,
) -> (Vec<Rename>, CollapseStats) {
    let component = rows[0].0;
    let umi_pairs = rows
        .iter()
        .map(|&(_, umi1, umi2)| (umi1 as UMI, umi2 as UMI));
    let umi_mapping = UmiToNodeIndexMapping::from_umi_pairs(umi_pairs.clone());
    let num_nodes = umi_mapping.get_num_of_nodes();
    let edges = umi_mapping.map_umi_pair_iterator_to_edge(umi_pairs);
    let graph = Graph::<u8>::from_edges(edges, num_nodes);

    let (targets, stats) = graph_collapse(
        &graph,
        &umi_mapping,
        bfs_threshold as usize,
        hamming_threshold,
        max_degree as usize,
    );
    let mut renames: Vec<Rename> = targets
        .into_iter()
        .map(|(original, corrected)| (component, original as u64, corrected as u64))
        .collect();
    renames.sort_unstable();
    (renames, stats)
}

/// Get a non-null UInt64 column by name.
///
/// Fails with a schema error if the column is missing, is not UInt64, or
/// contains a null.
fn uint64_column(batch: &RecordBatch, name: &str) -> Result<UInt64Array, ArrowError> {
    Ok(non_null_column(batch, name, &DataType::UInt64)?
        .as_primitive::<UInt64Type>()
        .clone())
}

/// Get a non-null UInt32 column by name.
///
/// Fails with a schema error if the column is missing, is not UInt32, or
/// contains a null.
fn uint32_column(batch: &RecordBatch, name: &str) -> Result<UInt32Array, ArrowError> {
    Ok(non_null_column(batch, name, &DataType::UInt32)?
        .as_primitive::<UInt32Type>()
        .clone())
}

/// Get a column by name, checking its type and that it has no nulls.
fn non_null_column<'a>(
    batch: &'a RecordBatch,
    name: &str,
    data_type: &DataType,
) -> Result<&'a dyn Array, ArrowError> {
    let column = batch
        .column_by_name(name)
        .ok_or_else(|| ArrowError::SchemaError(format!("Could not find {name} column in data")))?;
    if column.data_type() != data_type {
        return Err(ArrowError::SchemaError(format!(
            "Column {name} must be {data_type}, got {}",
            column.data_type()
        )));
    }
    if column.null_count() > 0 {
        return Err(ArrowError::SchemaError(format!(
            "Column {name} contains {} null values",
            column.null_count()
        )));
    }
    Ok(column.as_ref())
}

/// Build a single-batch table with non-null `component` (UInt32) and
/// `original` and `corrected` (UInt64) columns.
fn arrow_table(renames: Vec<Rename>) -> Result<Table, ArrowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("component", DataType::UInt32, false),
        Field::new("original", DataType::UInt64, false),
        Field::new("corrected", DataType::UInt64, false),
    ]));
    let (component, (original, corrected)): (Vec<u32>, (Vec<u64>, Vec<u64>)) = renames
        .into_iter()
        .map(|(component, original, corrected)| (component, (original, corrected)))
        .unzip();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(UInt32Array::from(component)),
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

    use super::*;

    fn umi(seq: &str) -> u64 {
        encode_dna_2bit(seq.as_bytes())
    }

    fn batch(component: &[u32], umi1: &[u64], umi2: &[u64]) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("umi1", DataType::UInt64, false),
            Field::new("umi2", DataType::UInt64, false),
            Field::new("component", DataType::UInt32, false),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(umi1.to_vec())),
                Arc::new(UInt64Array::from(umi2.to_vec())),
                Arc::new(UInt32Array::from(component.to_vec())),
            ],
        )
        .unwrap()
    }

    fn renames(table: &Table) -> Vec<Rename> {
        let batch = &table.record_batches()[0];
        let component = batch.column(0).as_primitive::<UInt32Type>().values();
        let original = batch.column(1).as_primitive::<UInt64Type>().values();
        let corrected = batch.column(2).as_primitive::<UInt64Type>().values();
        (0..batch.num_rows())
            .map(|row| (component[row], original[row], corrected[row]))
            .collect()
    }

    #[test]
    fn chain_collapses_within_its_component_only() {
        // A chain A -> B -> H in component 7. Its rows are split across
        // batches and interleaved with component 3, so grouping cannot rely on
        // the row order. CCAA is one symbol from CAAA, and CAAA is one symbol
        // from both CCAA and AAAA. Rows are sorted before the search, so CAAA
        // meets AAAA, the chain's end, first. Component 3 is one edge between
        // two distant UMIs, which has no collapse target. M and N are umi1 and
        // every other UMI is umi2, so none sits on both sides.
        let umi_m = umi("GGGG");
        let umi_h = umi("AAAA");
        let umi_a = umi("CCAA");
        let umi_b = umi("CAAA");
        let umi_n = umi("TTTT");
        let batches = [
            batch(&[7, 3], &[umi_m, umi("ACAC")], &[umi_h, umi("GTGT")]),
            batch(&[7, 7, 7], &[umi_m, umi_n, umi_m], &[umi_b, umi_h, umi_a]),
        ];

        let (table, stats) = run_graph_collapse(batches.into_iter().map(Ok), 2, 1, 1).unwrap();

        let mut expected = vec![(7, umi_a, umi_h), (7, umi_b, umi_h)];
        expected.sort_unstable();
        assert_eq!(renames(&table), expected);
        // Component 7 has three leaves (A, B and N) and component 3 has two.
        // Only the chain in component 7 finds matches.
        assert_eq!(stats.total_components, 2);
        assert_eq!(stats.nodes_considered, 5);
        assert_eq!(stats.targets_found, 2);
        assert_eq!(stats.bfs_distance_counts(), [(2, 2)]);
        assert_eq!(stats.hamming_distance_counts(), [(1, 2)]);
    }

    #[test]
    fn same_umis_in_two_components_collapse_separately() {
        // The same leaf chain appears in components 1 and 2. Each component is
        // its own graph, so each reports its own renames.
        let umi_m = umi("GGGG");
        let umi_h = umi("AAAA");
        let umi_a = umi("CAAA");
        let umi_n = umi("TTTT");
        let batches = [batch(
            &[2, 1, 2, 1, 2, 1],
            &[umi_m, umi_m, umi_m, umi_m, umi_n, umi_n],
            &[umi_h, umi_h, umi_a, umi_a, umi_h, umi_h],
        )];

        let (table, stats) = run_graph_collapse(batches.into_iter().map(Ok), 2, 1, 1).unwrap();

        assert_eq!(renames(&table), vec![(1, umi_a, umi_h), (2, umi_a, umi_h)]);
        assert_eq!(stats.total_components, 2);
        assert_eq!(stats.targets_found, 2);
    }

    #[test]
    fn a_component_of_another_type_is_a_schema_error() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("umi1", DataType::UInt64, false),
            Field::new("umi2", DataType::UInt64, false),
            Field::new("component", DataType::Utf8, false),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![1])),
                Arc::new(UInt64Array::from(vec![2])),
                Arc::new(arrow::array::StringArray::from(vec!["a"])),
            ],
        )
        .unwrap();

        let err = read_rows([Ok(batch)].into_iter()).unwrap_err();

        assert!(
            matches!(&err, ArrowError::SchemaError(message) if message == "Column component must be UInt32, got Utf8"),
            "{err:?}"
        );
    }

    #[test]
    fn a_null_component_is_a_schema_error() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("umi1", DataType::UInt64, false),
            Field::new("umi2", DataType::UInt64, false),
            Field::new("component", DataType::UInt32, true),
        ]));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(UInt64Array::from(vec![1, 3])),
                Arc::new(UInt64Array::from(vec![2, 4])),
                Arc::new(UInt32Array::from(vec![Some(0), None])),
            ],
        )
        .unwrap();

        let err = read_rows([Ok(batch)].into_iter()).unwrap_err();

        assert!(
            matches!(&err, ArrowError::SchemaError(message) if message == "Column component contains 1 null values"),
            "{err:?}"
        );
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
            vec!["component", "original", "corrected"]
        );
    }
}
