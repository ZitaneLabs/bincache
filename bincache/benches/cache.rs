use std::{borrow::Cow, hint::black_box, io::Write, path::Path, time::Duration};

use bincache::{
    Cache, CacheStrategy, CompressionStrategy, DiskStrategy, HybridStrategy, MemoryStrategy, Noop,
    RecoverableStrategy, strategies::Limits,
};
use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, async_executor::AsyncExecutor, criterion_group,
    criterion_main,
};

const SIZES: [usize; 3] = [1024, 64 * 1024, 1024 * 1024];
const OPERATIONS: usize = 16;

// Fixed inputs make separate runs comparable without another random-number dependency.
fn payload(size: usize, random: bool) -> Vec<u8> {
    let record = b"{\"id\":42,\"status\":\"ready\",\"tags\":[\"cache\",\"binary\"]}\n";
    let mut state = 0x4d595df4d0f33173_u64;
    (0..size)
        .map(|i| {
            if random {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            } else {
                record[i % record.len()]
            }
        })
        .collect()
}

fn temp_dir() -> tempfile::TempDir {
    let root = std::env::var_os("BINCACHE_BENCH_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    tempfile::Builder::new()
        .prefix("bincache-bench-")
        .tempdir_in(root)
        .unwrap()
}

fn fixture<S, C>(
    executor: &impl AsyncExecutor,
    strategy: &impl Fn(&Path) -> S,
    compressor: fn() -> Option<C>,
    data: &[u8],
    entries: usize,
) -> (Cache<usize, S, C>, tempfile::TempDir)
where
    S: CacheStrategy + Send,
    C: CompressionStrategy + Send + Sync,
{
    let dir = temp_dir();
    let mut cache = executor
        .block_on(Cache::new(strategy(dir.path()), compressor()))
        .unwrap();
    executor.block_on(async {
        for key in 0..entries {
            cache.put(key, data.to_vec()).await.unwrap();
        }
    });
    (cache, dir)
}

fn operations<S>(
    c: &mut Criterion,
    executor: &impl AsyncExecutor,
    name: &str,
    strategy: impl Fn(&Path) -> S,
) where
    S: CacheStrategy + Send,
{
    let mut group = c.benchmark_group(name);
    for size in SIZES {
        let data = payload(size, false);
        let empty = || fixture(executor, &strategy, || None::<Noop>, &data, 0);
        let seeded = || fixture(executor, &strategy, || None::<Noop>, &data, OPERATIONS);
        group.throughput(Throughput::Bytes((size * OPERATIONS) as u64));

        group.bench_function(BenchmarkId::new("put_sequential", size), |b| {
            b.iter_batched_ref(
                || (empty(), vec![data.clone(); OPERATIONS]),
                |((cache, _dir), values)| {
                    executor.block_on(async {
                        for (key, value) in values.drain(..).enumerate() {
                            cache.put(black_box(key), black_box(value)).await.unwrap();
                        }
                    });
                },
                BatchSize::PerIteration,
            );
        });

        let (mut cache, _dir) = seeded();
        assert_eq!(executor.block_on(cache.get(0)).unwrap().as_ref(), data);
        for repeated in [false, true] {
            let operation = if repeated {
                "get_repeated"
            } else {
                "get_sequential"
            };
            group.bench_function(BenchmarkId::new(operation, size), |b| {
                b.iter(|| {
                    executor.block_on(async {
                        for key in 0..OPERATIONS {
                            black_box(
                                cache
                                    .get(black_box(if repeated { 0 } else { key }))
                                    .await
                                    .unwrap(),
                            );
                        }
                    });
                });
            });
        }
        group.bench_function(BenchmarkId::new("replace_repeated", size), |b| {
            b.iter_batched_ref(
                || vec![data.clone(); OPERATIONS],
                |values| {
                    executor.block_on(async {
                        for value in values.drain(..) {
                            cache.put(black_box(0), black_box(value)).await.unwrap();
                        }
                    });
                },
                BatchSize::PerIteration,
            );
        });
        for take in [false, true] {
            let operation = if take {
                "take_sequential"
            } else {
                "delete_sequential"
            };
            group.bench_function(BenchmarkId::new(operation, size), |b| {
                b.iter_batched_ref(
                    seeded,
                    |(cache, _dir)| {
                        executor.block_on(async {
                            for key in 0..OPERATIONS {
                                if take {
                                    black_box(cache.take(black_box(key)).await.unwrap());
                                } else {
                                    cache.delete(black_box(key)).await.unwrap();
                                }
                            }
                        });
                    },
                    BatchSize::PerIteration,
                );
            });
        }
    }
    group.finish();
}

fn recovery<S>(
    c: &mut Criterion,
    executor: &impl AsyncExecutor,
    name: &str,
    strategy: impl Fn(&Path) -> S,
) where
    S: RecoverableStrategy + Send,
{
    let mut group = c.benchmark_group(name);
    group.throughput(Throughput::Elements(OPERATIONS as u64));
    for size in SIZES {
        let data = payload(size, false);
        let (cache, dir) = fixture(executor, &strategy, || None::<Noop>, &data, OPERATIONS);
        drop(cache);
        group.bench_function(BenchmarkId::new("recover", size), |b| {
            b.iter_batched_ref(
                || {
                    executor
                        .block_on(Cache::<usize, _, _>::new(
                            strategy(dir.path()),
                            None::<Noop>,
                        ))
                        .unwrap()
                },
                |cache| {
                    let count = executor
                        .block_on(cache.recover(|key| key.parse().ok()))
                        .unwrap();
                    assert_eq!(black_box(count), OPERATIONS);
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn compressed_cache<S, C>(
    c: &mut Criterion,
    executor: &impl AsyncExecutor,
    name: &str,
    strategy: impl Fn(&Path) -> S,
    compressor: fn() -> Option<C>,
    data: &[u8],
) where
    S: CacheStrategy + Send,
    C: CompressionStrategy + Send + Sync,
{
    let mut group = c.benchmark_group(name);
    group.throughput(Throughput::Bytes(data.len() as u64));
    group.bench_function(BenchmarkId::new("put", data.len()), |b| {
        b.iter_batched_ref(
            || {
                (
                    fixture(executor, &strategy, compressor, data, 0),
                    data.to_vec(),
                )
            },
            |((cache, _dir), value)| {
                executor
                    .block_on(cache.put(black_box(0), black_box(std::mem::take(value))))
                    .unwrap();
            },
            BatchSize::PerIteration,
        );
    });
    let (cache, _dir) = fixture(executor, &strategy, compressor, data, 1);
    // Validate every codec, runtime, payload and size before measuring reads.
    assert_eq!(executor.block_on(cache.get(0)).unwrap().as_ref(), data);
    group.bench_function(BenchmarkId::new("get_repeated", data.len()), |b| {
        b.iter(|| black_box(executor.block_on(cache.get(black_box(0))).unwrap()));
    });
    group.finish();
}

fn compression<C>(
    c: &mut Criterion,
    executor: &impl AsyncExecutor,
    name: &str,
    compressor: fn() -> Option<C>,
    sizes: &mut std::fs::File,
) where
    C: CompressionStrategy + Send + Sync,
{
    for (pattern, random) in [("text", false), ("random", true)] {
        for size in SIZES {
            let data = payload(size, random);
            let codec = compressor();
            let encoded = executor
                .block_on(codec.compress(Cow::Borrowed(&data)))
                .unwrap();
            assert!(
                executor
                    .block_on(codec.decompress(encoded.clone()))
                    .unwrap()
                    .as_ref()
                    == data,
                "{name}/{pattern}: round trip failed for {size} bytes",
            );

            let (cache, dir) = fixture(
                executor,
                &|path| DiskStrategy::new(path, None, None),
                compressor,
                &data,
                1,
            );
            let file_bytes: u64 = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().metadata().unwrap().len())
                .sum();
            writeln!(
                sizes,
                "{name},{pattern},{size},{},{file_bytes},{:.6}",
                encoded.len(),
                encoded.len() as f64 / size as f64
            )
            .unwrap();
            drop(cache);

            let mut group = c.benchmark_group(format!("{name}/{pattern}/codec"));
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_function(BenchmarkId::new("compress", size), |b| {
                b.iter(|| {
                    black_box(
                        executor
                            .block_on(codec.compress(Cow::Borrowed(black_box(&data))))
                            .unwrap(),
                    )
                });
            });
            group.bench_function(BenchmarkId::new("decompress", size), |b| {
                b.iter(|| {
                    black_box(
                        executor
                            .block_on(codec.decompress(Cow::Borrowed(black_box(encoded.as_ref()))))
                            .unwrap(),
                    )
                });
            });
            group.finish();

            compressed_cache(
                c,
                executor,
                &format!("{name}/{pattern}/memory"),
                |_| MemoryStrategy::default(),
                compressor,
                &data,
            );
            compressed_cache(
                c,
                executor,
                &format!("{name}/{pattern}/disk"),
                |path| DiskStrategy::new(path, None, None),
                compressor,
                &data,
            );
        }
    }
}

fn suite(c: &mut Criterion, executor: &impl AsyncExecutor, runtime: &str) {
    operations(c, executor, &format!("{runtime}/memory"), |_| {
        MemoryStrategy::default()
    });
    operations(c, executor, &format!("{runtime}/disk"), |path| {
        DiskStrategy::new(path, None, None)
    });
    operations(c, executor, &format!("{runtime}/hybrid-memory"), |path| {
        HybridStrategy::new(path, Limits::default(), Limits::default())
    });
    operations(c, executor, &format!("{runtime}/hybrid-disk"), |path| {
        HybridStrategy::new(path, Limits::new(None, Some(0)), Limits::default())
    });
    operations(c, executor, &format!("{runtime}/hybrid-mixed"), |path| {
        HybridStrategy::new(
            path,
            Limits::new(None, Some(OPERATIONS / 2)),
            Limits::default(),
        )
    });
    recovery(c, executor, &format!("{runtime}/disk"), |path| {
        DiskStrategy::new(path, None, None)
    });
    recovery(c, executor, &format!("{runtime}/hybrid-disk"), |path| {
        HybridStrategy::new(path, Limits::new(None, Some(0)), Limits::default())
    });

    let report_dir = std::env::var_os("BINCACHE_BENCH_REPORT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/criterion"));
    std::fs::create_dir_all(&report_dir).unwrap();
    let mut sizes =
        std::fs::File::create(report_dir.join(format!("compression-sizes-{runtime}.csv"))).unwrap();
    writeln!(
        sizes,
        "runtime/codec,pattern,input_bytes,stored_payload_bytes,disk_file_bytes,stored_over_input"
    )
    .unwrap();
    compression(
        c,
        executor,
        &format!("{runtime}/none"),
        || None::<Noop>,
        &mut sizes,
    );
    #[cfg(feature = "comp_gzip")]
    compression(
        c,
        executor,
        &format!("{runtime}/gzip-default"),
        || Some(bincache::compression::Gzip::default()),
        &mut sizes,
    );
    #[cfg(feature = "comp_brotli")]
    compression(
        c,
        executor,
        &format!("{runtime}/brotli-default"),
        || Some(bincache::compression::Brotli::default()),
        &mut sizes,
    );
    #[cfg(feature = "comp_zstd")]
    compression(
        c,
        executor,
        &format!("{runtime}/zstd-default"),
        || Some(bincache::compression::Zstd::default()),
        &mut sizes,
    );
}

fn benchmarks(c: &mut Criterion) {
    #[cfg(feature = "rt_tokio_1")]
    suite(
        c,
        &tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
        "tokio",
    );
    #[cfg(feature = "rt_async-std_1")]
    suite(c, &criterion::async_executor::AsyncStdExecutor, "async-std");
    #[cfg(not(any(feature = "rt_tokio_1", feature = "rt_async-std_1")))]
    suite(c, &criterion::async_executor::FuturesExecutor, "blocking");
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(3));
    targets = benchmarks
}
criterion_main!(benches);
