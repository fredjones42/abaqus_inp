use std::fmt::Write;
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

/// k×k×k node grid of C3D8 hex elements, as .inp text.
fn synthetic(k: u32) -> String {
    let id = |x, y, z| x + k * (y + k * z) + 1;
    let mut s = String::from("*NODE\n");
    for z in 0..k {
        for y in 0..k {
            for x in 0..k {
                writeln!(s, "{}, {x}.0, {y}.0, {z}.0", id(x, y, z)).unwrap();
            }
        }
    }
    s.push_str("*ELEMENT, TYPE=C3D8, ELSET=all\n");
    let mut e = 1;
    for z in 0..k - 1 {
        for y in 0..k - 1 {
            for x in 0..k - 1 {
                let n = [
                    id(x, y, z),
                    id(x + 1, y, z),
                    id(x + 1, y + 1, z),
                    id(x, y + 1, z),
                    id(x, y, z + 1),
                    id(x + 1, y, z + 1),
                    id(x + 1, y + 1, z + 1),
                    id(x, y + 1, z + 1),
                ];
                writeln!(
                    s,
                    "{e}, {}, {}, {}, {}, {}, {}, {}, {}",
                    n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7]
                )
                .unwrap();
                e += 1;
            }
        }
    }
    s
}

fn bench_parse(c: &mut Criterion) {
    let uuea = include_str!("../tests/data/UUea.inp");
    c.bench_function("UUea", |b| {
        b.iter(|| abaqus_inp::parse_str(black_box(uuea)).unwrap())
    });

    let big = synthetic(40); // 64k nodes, 59k hexes, ~4 MB
    let mut g = c.benchmark_group("synthetic");
    g.throughput(Throughput::Bytes(big.len() as u64));
    g.bench_function("hex_40", |b| {
        b.iter(|| abaqus_inp::parse_str(black_box(&big)).unwrap())
    });
    g.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
