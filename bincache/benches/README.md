# Reproducible benchmarks

Run from the repository root. Criterion 0.7 is used to retain Rust 1.85 support;
the lockfile pins the complete dependency graph. The benchmark target uses
release optimizations. No numerical results are checked in: measure on the
hardware, filesystem, and inputs relevant to your application.

## Run each I/O mode separately

```sh
cargo bench --locked --bench cache --no-default-features --features blocking,comp_gzip,comp_brotli,comp_zstd
cargo bench --locked --bench cache --no-default-features --features rt_tokio_1,comp_gzip,comp_brotli,comp_zstd
cargo bench --locked --bench cache --no-default-features --features rt_async-std_1,comp_gzip,comp_brotli,comp_zstd
```

The default feature set also works and measures blocking I/O with no compression.
Do not use `--all-features`: the explicit I/O modes are mutually exclusive.

Each full run has hundreds of cases and can take tens of minutes. Select a
subset using a Criterion filter; runtime, strategy/codec, operation and input
size (in bytes) appear in benchmark identifiers:

```sh
cargo bench --locked --bench cache --features comp_gzip,comp_brotli,comp_zstd -- 'blocking/memory/get_repeated/65536'
cargo bench --locked --bench cache --features comp_gzip,comp_brotli,comp_zstd -- 'blocking/(none|gzip-default|brotli-default|zstd-default)/text/disk'
```

Compile or execute each case once without collecting performance measurements:

```sh
cargo bench --locked --bench cache --features comp_gzip,comp_brotli,comp_zstd --no-run
cargo bench --locked --bench cache --features comp_gzip,comp_brotli,comp_zstd -- --test
```

CI performs both checks for blocking, Tokio and async-std. Round-trip and recovery
assertions fail the smoke test if the workloads stop behaving as intended.

## Workloads and timing boundaries

All cases use 1 KiB, 64 KiB and 1 MiB **uncompressed** payloads.

| Group | Measured work per Criterion iteration |
| --- | --- |
| `memory`, `disk` | 16 sequential inserts, reads, takes, or deletes; 16 reads/replacements of key 0 |
| `hybrid-memory` | Same operations, unlimited memory: every entry is in RAM |
| `hybrid-disk` | Same operations, memory entry limit zero: every entry is on disk |
| `hybrid-mixed` | Same operations, memory entry limit eight: first eight keys in RAM, remaining eight on disk; repeated reads/replacements use memory key 0 |
| `disk/recover`, `hybrid-disk/recover` | Recover 16 existing files into a fresh cache, without decompression |
| `<codec>/<pattern>/codec` | One complete compression or decompression |
| `<codec>/<pattern>/memory`, `<codec>/<pattern>/disk` | One cache insert or repeated read, including the codec |

Core operation cases are uncompressed. Their latency is **per batch of 16**;
divide by 16 for average latency per operation. Recovery latency is per directory
of 16 files; its throughput is entries/second. Compression groups measure one
operation; byte throughput always refers to original payload bytes. Delete
throughput describes removed logical bytes, not bytes transferred to disk.

Input generation, cache/directory construction, seeding, and input `Vec` cloning
are outside timed regions. Insert/take/delete cases use fresh fixtures so they
never drift into replacement/missing-key operations or grow with iteration count.
`iter_batched_ref` with `PerIteration` bounds live fixture storage and keeps
cache/directory destruction outside timing. Replacement overwrites a seeded key
with the same payload repeatedly, at a stable cache size. Output destruction from
reads/takes/codecs and disposal of previous replacement values is included.
These boundaries follow [Criterion's timing-loop guidance](https://bheisler.github.io/criterion.rs/book/user_guide/timing_loops.html).

The executor is created outside measurement and reused: FuturesExecutor for
blocking I/O, a current-thread Tokio runtime, or async-std's global executor.
Each timed batch/operation includes `block_on` overhead. Very fast memory results
therefore include executor, async-trait dispatch, and harness overhead; they do
not isolate HashMap lookup time. Workloads issue one operation at a time, with
no task contention or parallel clients. The three builds select different file
I/O implementations, not a claim that all work is nonblocking: recovery directory
traversal and rename still use synchronous calls, and compression uses caller CPU.

Disk writes include bincache's file-data sync and rename. Reads and recovery are
warm/repeated workloads affected by the OS page cache. No page-cache dropping,
direct I/O, cold-start latency, crash injection, or durability test is performed.
Recovery reuses a valid directory but creates a fresh strategy/index each time;
it includes reading full files, key parsing and index reconstruction, not startup
directory creation or validation by decompression.

## Compression latency and stored sizes

`none` is the same no-compression path used by a cache built without a compressor.
Gzip, Brotli and Zstandard are benchmarked at their underlying default levels.
Those levels do not imply equivalent quality or CPU budgets.

Each codec uses two deterministic inputs:

- `text`: a repeated JSON-like record, deliberately highly compressible.
- `random`: fixed-seed xorshift bytes, representing poorly compressible data.

Neither is a substitute for your real data. Raw codec groups separate CPU/buffer
costs from cache/index/filesystem work; memory/disk groups show end-to-end writes
and reads. Hybrid uses the same codec before placement/after reads, so codec
comparisons focus on the two underlying storage paths. Size reductions can also
change which hybrid tier a production value fits into.

Every invocation, including filtered runs and `--test`, writes
`target/criterion/compression-sizes-<runtime>.csv` relative to the workspace.
It includes all enabled codecs and input cases, independently of the timing
filter. Copy this report before another run of the same mode overwrites it.

| CSV column | Meaning |
| --- | --- |
| `runtime/codec`, `pattern` | I/O mode, algorithm/level and deterministic corpus |
| `input_bytes` | Original payload length |
| `stored_payload_bytes` | Actual encoded payload length, the quantity used by byte limits |
| `disk_file_bytes` | Sum of actual file lengths for one cached key, including bincache's header/key bytes |
| `stored_over_input` | Encoded/original bytes; below 1 means smaller, above 1 means expansion |

The uncompressed baseline uses `None`, not an identity codec wrapped in `Some`.
File length excludes filesystem blocks, directory entries, transient write files,
and allocation overhead. The size report is generated outside measurements and
verifies round trips before reporting sizes. Compare write latency, read latency,
and stored bytes together; no codec is universally preferable.

## Reproduce and compare

The default sampling is 20 samples, one second warm-up, and three seconds requested
measurement time per case. Criterion may extend sampling for slow operations.
Record the commit, `Cargo.lock`, `rustc -Vv`, command/features, CPU, OS, filesystem,
storage device, power settings, free disk space and background workload. Keep
those conditions fixed across runs. Avoid concurrent benchmark jobs.

Temporary cache directories are unique and automatically removed. By default
they use the OS temporary directory, which may be tmpfs. To measure a specific
device, set `BINCACHE_BENCH_DIR` to an **existing directory on that filesystem**.
Size-report output defaults to workspace `target/criterion` regardless of
`CARGO_TARGET_DIR`; override it with `BINCACHE_BENCH_REPORT_DIR` if needed.
Criterion timing estimates/baselines go to its normal `target/criterion` output
(or its configured output directory, e.g. `CRITERION_HOME`).

Save a focused baseline, then compare after a change using exactly the same
command, toolchain, input and environment:

```sh
cargo bench --locked --bench cache -- 'blocking/memory/get_repeated/65536' --save-baseline before
# After changing the implementation:
cargo bench --locked --bench cache -- 'blocking/memory/get_repeated/65536' --baseline before
```

Retain Criterion estimates and the size CSV with environment notes when sharing
results. Repeat surprising differences; statistical significance alone does not
establish practical importance, especially for noisy filesystem measurements.

## Extending the suite

`SIZES` and `payload` define inputs. Add a strategy factory to `suite` to reuse
`operations` and, if supported, `recovery`. Register another codec/level through
`compression`; it automatically gets latency, cache, size and round-trip cases.
Select another `AsyncExecutor` in `benchmarks` and add its feature set to CI when
introducing a new I/O mode. Keep benchmark IDs and fixture placement explicit so
existing baselines remain interpretable.
