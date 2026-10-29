use divan::AllocProfiler;
use mimalloc::MiMalloc;

#[global_allocator]
static ALLOC: AllocProfiler<MiMalloc> = AllocProfiler::new(MiMalloc);

fn main() {
    // Run registered benchmarks.
    divan::main();
}

mod bench_community_detection;
mod bench_connected_components;
mod bench_edgelist;
mod bench_graph_collapse;
mod bench_memory;
mod bench_parquet_io;
