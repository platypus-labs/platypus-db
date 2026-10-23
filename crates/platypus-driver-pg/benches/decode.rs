//! Throughput of wire-format decoding into Arrow builders, without a database.

use std::hint::black_box;

use arrow_array::builder::{Int64Builder, StringBuilder};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use platypus_driver_pg::columns::ColumnBuilder;
use platypus_driver_pg::decode::{format_value, numeric_from_sql};
use tokio_postgres::types::Type;

const ROWS: u64 = 100_000;

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");
    group.throughput(Throughput::Elements(ROWS));

    let int8 = 42i64.to_be_bytes();
    group.bench_function("int8", |b| {
        b.iter(|| {
            let mut builder = ColumnBuilder::Int64(Int64Builder::new());
            for _ in 0..ROWS {
                builder.append(Some(black_box(&int8))).unwrap();
            }
            builder.finish()
        });
    });

    let text = b"5d41402abc4b2a76b9719d911017c592";
    group.bench_function("text", |b| {
        b.iter(|| {
            let mut builder = ColumnBuilder::Text(StringBuilder::new());
            for _ in 0..ROWS {
                builder.append(Some(black_box(text))).unwrap();
            }
            builder.finish()
        });
    });

    let ts = 821_100_000_000_000i64.to_be_bytes();
    group.bench_function("timestamp", |b| {
        b.iter(|| {
            let mut builder = ColumnBuilder::timestamp(false);
            for _ in 0..ROWS {
                builder.append(Some(black_box(&ts))).unwrap();
            }
            builder.finish()
        });
    });

    // 12345678.9012
    let numeric: Vec<u8> = [3i16, 1, 0, 4, 1234, 5678, 9012]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    group.bench_function("numeric_to_decimal128", |b| {
        b.iter(|| {
            for _ in 0..ROWS {
                black_box(numeric_from_sql(black_box(&numeric)).unwrap().to_i128(4));
            }
        });
    });
    group.bench_function("timestamp_formatted", |b| {
        b.iter(|| {
            for _ in 0..ROWS {
                black_box(format_value(&Type::TIMESTAMP, black_box(&ts)).unwrap());
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
