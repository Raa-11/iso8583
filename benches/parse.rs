//! New API (`Message` / `Builder`) vs the published 0.1.1 (`IsoStruct`) on the same message
//! (fields 2, 3, 4, 7, 11, 12, 13, 37, 41, 42, 49).

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use iso_8583_old::iso8583::new_iso_struct as old_new_iso_struct;
use iso_8583_rs::{Builder, CompiledSpec, Message};
use std::hint::black_box;

const SPEC: &str = "spec1987.yml";

/// 0.1.1 requires `MinLen` on every field, while the spec file (like the original Go one) omits
/// it on many. Write a copy with `MinLen: 0` added where missing, for 0.1.1 only.
fn spec_for_0_1_1() -> String {
    let src = std::fs::read_to_string(SPEC).unwrap();
    let mut out = String::new();
    let mut block = String::new();
    let flush = |out: &mut String, block: &mut String| {
        if !block.is_empty() && !block.contains("MinLen") {
            block.push_str("  MinLen: 0\n");
        }
        out.push_str(block);
        block.clear();
    };
    for line in src.lines() {
        if line.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            flush(&mut out, &mut block);
        }
        block.push_str(line);
        block.push('\n');
    }
    flush(&mut out, &mut block);
    let path = std::env::temp_dir().join("iso8583_bench_spec_0_1_1.yml");
    std::fs::write(&path, out).unwrap();
    path.to_str().unwrap().to_owned()
}

const FIELDS: [(usize, &str); 11] = [
    (2, "4111111111111111"),
    (3, "000000"),
    (4, "000000010000"),
    (7, "0924120000"),
    (11, "123456"),
    (12, "120000"),
    (13, "0924"),
    (37, "123456789012"),
    (41, "TERM0001"),
    (42, "MERCHANT0000001"),
    (49, "360"),
];

fn bench(c: &mut Criterion) {
    let spec = CompiledSpec::from_file(SPEC).unwrap();

    // --- new
    let mut b = Builder::new(&spec, b"0200").unwrap();
    for (n, v) in FIELDS {
        b.set(n, v.as_bytes()).unwrap();
    }
    let mut buf = Vec::new();
    b.pack_into(&mut buf);

    // --- published 0.1.1
    let mut legacy = old_new_iso_struct(&spec_for_0_1_1(), false).unwrap();
    legacy.add_mti("0200").unwrap();
    for (n, v) in FIELDS {
        legacy.add_field(n as i64, v).unwrap();
    }
    let legacy_str = legacy.to_string().unwrap();
    assert_eq!(
        legacy_str.to_uppercase(),
        String::from_utf8(buf.clone()).unwrap().to_uppercase()
    );

    let mut g = c.benchmark_group("parse");
    g.throughput(Throughput::Elements(1));
    g.bench_function("new strict", |bn| {
        bn.iter(|| Message::parse(&spec, black_box(&buf)).unwrap().get(4))
    });
    g.bench_function("new lazy", |bn| {
        bn.iter(|| Message::parse_lazy(&spec, black_box(&buf)).unwrap().get(4))
    });
    g.bench_function("0.1.1 IsoStruct", |bn| {
        bn.iter(|| legacy.parse(black_box(&legacy_str)).unwrap())
    });
    g.finish();

    let mut g = c.benchmark_group("pack");
    g.throughput(Throughput::Elements(1));
    let mut out = Vec::with_capacity(512);
    g.bench_function("new build+pack_into", |bn| {
        bn.iter(|| {
            let mut b = Builder::new(&spec, b"0200").unwrap();
            for (n, v) in FIELDS {
                b.set(n, black_box(v.as_bytes())).unwrap();
            }
            b.pack_into(&mut out);
            out.len()
        })
    });
    g.bench_function("new pack_into only", |bn| {
        bn.iter(|| {
            black_box(&b).pack_into(&mut out);
            out.len()
        })
    });
    g.bench_function("0.1.1 to_string", |bn| {
        bn.iter(|| black_box(&legacy).to_string().unwrap())
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
