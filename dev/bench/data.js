window.BENCHMARK_DATA = {
  "lastUpdate": 1793379014265,
  "repoUrl": "https://github.com/Aratz/pixelator-core",
  "entries": {
    "Native Community Detection Benchmark": [
      {
        "commit": {
          "author": {
            "email": "adrien.coulier@pixelgen.com",
            "name": "Adrien Coulier",
            "username": "Aratz"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "6847aabab87bedbe111e3688bae8cb7116b6a903",
          "message": "ci: build wheels on release PRs (#6)\n\nCo-authored-by: Cursor <cursoragent@cursor.com>",
          "timestamp": "2026-10-30T16:54:58+01:00",
          "tree_id": "5dd8982ca890ed382b6f08edb46ab49bcc278b9f",
          "url": "https://github.com/Aratz/pixelator-core/commit/6847aabab87bedbe111e3688bae8cb7116b6a903"
        },
        "date": 1793376055034,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "bench_fast_label_propagation",
            "value": 0.004984,
            "unit": "s"
          },
          {
            "name": "bench_leiden_modularity",
            "value": 0.01502,
            "unit": "s"
          },
          {
            "name": "bench_leiden_modularity_medium",
            "value": 11.58,
            "unit": "s"
          },
          {
            "name": "bench_connected_components",
            "value": 0.12040000000000001,
            "unit": "s"
          },
          {
            "name": "bench_edgelist_paired",
            "value": 0.3119,
            "unit": "s"
          },
          {
            "name": "bench_bounded_even_breadth_first_search/2",
            "value": 0.08892,
            "unit": "s"
          },
          {
            "name": "bench_bounded_even_breadth_first_search/4",
            "value": 0.8278,
            "unit": "s"
          },
          {
            "name": "bench_bounded_even_breadth_first_search/6",
            "value": 6.43,
            "unit": "s"
          },
          {
            "name": "bench_find_collapse_target/2",
            "value": 0.1154,
            "unit": "s"
          },
          {
            "name": "bench_find_collapse_target/4",
            "value": 0.8581000000000001,
            "unit": "s"
          },
          {
            "name": "bench_find_collapse_target/6",
            "value": 5.927,
            "unit": "s"
          },
          {
            "name": "bench_graph_collapse/2",
            "value": 0.08220000000000001,
            "unit": "s"
          },
          {
            "name": "bench_graph_collapse/4",
            "value": 0.5927,
            "unit": "s"
          },
          {
            "name": "bench_graph_collapse/6",
            "value": 3.868,
            "unit": "s"
          },
          {
            "name": "mem_fast_label_propagation",
            "value": 53.2532,
            "unit": "MiB"
          },
          {
            "name": "mem_graph_construction",
            "value": 342.9413,
            "unit": "MiB"
          },
          {
            "name": "mem_leiden_modularity",
            "value": 564.6706,
            "unit": "MiB"
          },
          {
            "name": "bench_create_graph_from_parquet",
            "value": 0.9873,
            "unit": "s"
          },
          {
            "name": "bench_parquet_reading",
            "value": 0.2556,
            "unit": "s"
          },
          {
            "name": "bench_parquet_writing",
            "value": 1.072,
            "unit": "s"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "adrien.coulier@pixelgen.com",
            "name": "Adrien Coulier",
            "username": "Aratz"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "2eb5c59a8f5f559c76b401a27c2f71663944216b",
          "message": "chore(dev): release 1.0.3 (#10)",
          "timestamp": "2026-10-30T17:43:56+01:00",
          "tree_id": "13f646834e208ce0596ddb616869aa0ff6255edf",
          "url": "https://github.com/Aratz/pixelator-core/commit/2eb5c59a8f5f559c76b401a27c2f71663944216b"
        },
        "date": 1793379011377,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "bench_fast_label_propagation",
            "value": 0.004683,
            "unit": "s"
          },
          {
            "name": "bench_leiden_modularity",
            "value": 0.01884,
            "unit": "s"
          },
          {
            "name": "bench_leiden_modularity_medium",
            "value": 11.77,
            "unit": "s"
          },
          {
            "name": "bench_connected_components",
            "value": 0.2554,
            "unit": "s"
          },
          {
            "name": "bench_edgelist_paired",
            "value": 0.3946,
            "unit": "s"
          },
          {
            "name": "bench_bounded_even_breadth_first_search/2",
            "value": 0.1188,
            "unit": "s"
          },
          {
            "name": "bench_bounded_even_breadth_first_search/4",
            "value": 0.9607000000000001,
            "unit": "s"
          },
          {
            "name": "bench_bounded_even_breadth_first_search/6",
            "value": 7.652,
            "unit": "s"
          },
          {
            "name": "bench_find_collapse_target/2",
            "value": 0.12819999999999998,
            "unit": "s"
          },
          {
            "name": "bench_find_collapse_target/4",
            "value": 0.9676,
            "unit": "s"
          },
          {
            "name": "bench_find_collapse_target/6",
            "value": 6.983,
            "unit": "s"
          },
          {
            "name": "bench_graph_collapse/2",
            "value": 0.06984,
            "unit": "s"
          },
          {
            "name": "bench_graph_collapse/4",
            "value": 0.4265,
            "unit": "s"
          },
          {
            "name": "bench_graph_collapse/6",
            "value": 3.043,
            "unit": "s"
          },
          {
            "name": "mem_fast_label_propagation",
            "value": 53.2532,
            "unit": "MiB"
          },
          {
            "name": "mem_graph_construction",
            "value": 342.9413,
            "unit": "MiB"
          },
          {
            "name": "mem_leiden_modularity",
            "value": 564.6706,
            "unit": "MiB"
          },
          {
            "name": "bench_create_graph_from_parquet",
            "value": 1.133,
            "unit": "s"
          },
          {
            "name": "bench_parquet_reading",
            "value": 0.3575,
            "unit": "s"
          },
          {
            "name": "bench_parquet_writing",
            "value": 1.51,
            "unit": "s"
          }
        ]
      }
    ]
  }
}