use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use divan::Bencher;
use divan::counter::ItemsCount;
use pixelator_core::edgelist::{FastqInput, FilterConfig, edgelist};
use pixelator_core::test_fixtures::{
    OwnedRecord, ReadMixConfig, proxiome_v2_assay, proxiome_v2_panel, proxiome_v2_read_pairs,
};

/// Paired reads per measured run. Matches the Python `run_edgelist` bench.
const N_READS: usize = 1_000_000;
/// Worker threads. Matches the Python `run_edgelist` bench.
const N_THREADS: usize = 8;

/// Serializes one side of a set of read pairs into an in-memory FASTQ buffer.
fn records_to_fastq<'a>(records: impl Iterator<Item = &'a OwnedRecord>) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, rec) in records.enumerate() {
        out.extend_from_slice(format!("@read{i}\n").as_bytes());
        out.extend_from_slice(&rec.seq);
        out.push(b'\n');
        out.extend_from_slice(b"+\n");
        out.extend_from_slice(&rec.qual);
        out.push(b'\n');
    }
    out
}

/// A fully valid mix, so every generated read produces one output row.
fn valid_mix() -> ReadMixConfig {
    ReadMixConfig {
        frac_has_n: 0.0,
        frac_unknown_pid: 0.0,
        read_len: 150,
    }
}

/// Full paired-end pipeline: FASTQ parse, extraction, and Parquet write.
///
/// Read generation happens once and is not timed. Each sample rebuilds the
/// readers and the output directory, then times only `edgelist`.
#[divan::bench(sample_count = 5, sample_size = 1)]
fn bench_edgelist_paired(bencher: Bencher) {
    let assay = proxiome_v2_assay();
    let panel = proxiome_v2_panel();
    let pairs = proxiome_v2_read_pairs(N_READS, valid_mix(), 0);
    let r1 = records_to_fastq(pairs.iter().map(|(left, _)| left));
    let r2 = records_to_fastq(pairs.iter().map(|(_, right)| right));

    bencher
        .with_inputs(|| {
            let tmp = tempfile::tempdir().expect("create tempdir");
            let input = FastqInput::Paired {
                r1: Box::new(Cursor::new(r1.clone())),
                r2: Box::new(Cursor::new(r2.clone())),
            };
            (tmp, input, assay.clone(), panel.clone())
        })
        .counter(ItemsCount::new(N_READS))
        .bench_local_values(|(tmp, input, assay, panel)| {
            edgelist(
                input,
                "sample".to_string(),
                assay,
                panel,
                tmp.path(),
                N_THREADS,
                FilterConfig::default(),
                false,
                Arc::new(AtomicU64::new(0)),
            )
            .expect("edgelist run");
        });
}
