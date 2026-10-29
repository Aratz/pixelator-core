use divan::Bencher;
use pixelator_core::common::io::{
    ParquetUMIPairIter, create_graph_and_umi_mapping_from_parquet_file,
    write_node_partitions_to_parquet,
};
use pixelator_core::common::node_indexing::UmiToNodeIndexMapping;
use pixelator_core::common::node_partitioning::{FastNodePartitioning, NodePartitioning};
use std::path::PathBuf;
use tempfile::NamedTempFile;

#[divan::bench(sample_count = 10)]
fn bench_parquet_reading(bencher: Bencher) {
    let mut parquet_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    parquet_path.push("test_data/mix_40cells_0pc.parquet");

    bencher.bench_local(move || {
        let _edge_iter: Vec<_> = ParquetUMIPairIter::new(
            parquet_path
                .to_str()
                .expect("Failed to convert PathBuf to &str"),
        )
        .expect("Failed to create ParquetEdgeIter")
        .collect();
    });
}

#[divan::bench(sample_count = 10)]
fn bench_parquet_writing(bencher: Bencher) {
    let tmp_file = NamedTempFile::new().expect("Failed to create tmp file");
    let path = tmp_file.path();
    let n = 10000000;
    let random_umi_pairs: Vec<(usize, usize)> = (0..n).map(|i| (i, (i + 1) % n)).collect();

    let node_partitioning = FastNodePartitioning::initialize_with_singlet_partitions(n);
    let mapping = UmiToNodeIndexMapping::from_umi_pairs(random_umi_pairs.iter().copied());

    bencher.bench_local(move || {
        let res = write_node_partitions_to_parquet(path, &node_partitioning, &mapping, None);
        assert!(
            res.is_ok(),
            "Failed to write node partitions to parquet: {:?}",
            res.err()
        );
    });
}

#[divan::bench(sample_count = 10)]
fn bench_create_graph_from_parquet(bencher: Bencher) {
    let mut parquet_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    parquet_path.push("test_data/mix_40cells_0pc.parquet");

    bencher.bench_local(move || {
        let _ = create_graph_and_umi_mapping_from_parquet_file::<u8>(
            parquet_path
                .to_str()
                .expect("Failed to convert PathBuf to &str"),
        );
    });
}
