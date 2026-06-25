use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use ecs::sparse::SparseSet;
use std::collections::HashMap;

// ── insert ──────────────────────────────────────

fn bench_insert(c: &mut Criterion) {
    let mut g = c.benchmark_group("insert");
    for &size in &[100u32, 1_000, 10_000, 100_000] {
        g.bench_with_input(BenchmarkId::new("SparseSet", size), &size, |b, &size| {
            b.iter(|| {
                let mut set: SparseSet<u32, u64> = SparseSet::with_capacity(size as usize);
                for i in 0..size {
                    set.insert(i, i as u64);
                }
                black_box(set);
            });
        });
        g.bench_with_input(BenchmarkId::new("HashMap", size), &size, |b, &size| {
            b.iter(|| {
                let mut map = HashMap::with_capacity(size as usize);
                for i in 0..size {
                    map.insert(i, i as u64);
                }
                black_box(map);
            });
        });
    }
    g.finish();
}

// ── get（命中） ─────────────────────────────────

fn bench_get_hit(c: &mut Criterion) {
    let mut g = c.benchmark_group("get_hit");
    for &size in &[100u32, 1_000, 10_000, 100_000] {
        // 预填充
        let mut sparse: SparseSet<u32, u64> = SparseSet::with_capacity(size as usize);
        let mut hash = HashMap::with_capacity(size as usize);
        for i in 0..size {
            sparse.insert(i, i as u64);
            hash.insert(i, i as u64);
        }

        g.bench_with_input(BenchmarkId::new("SparseSet", size), &size, |b, _| {
            b.iter(|| {
                let mut sum = 0u64;
                for i in 0..size {
                    sum += sparse.get(&i).unwrap();
                }
                black_box(sum);
            });
        });
        g.bench_with_input(BenchmarkId::new("HashMap", size), &size, |b, _| {
            b.iter(|| {
                let mut sum = 0u64;
                for i in 0..size {
                    sum += hash.get(&i).unwrap();
                }
                black_box(sum);
            });
        });
    }
    g.finish();
}

// ── get（未命中） ───────────────────────────────

fn bench_get_miss(c: &mut Criterion) {
    let mut g = c.benchmark_group("get_miss");
    for &size in &[100u32, 1_000, 10_000, 100_000] {
        let mut sparse: SparseSet<u32, u64> = SparseSet::with_capacity(size as usize);
        let mut hash = HashMap::with_capacity(size as usize);
        for i in 0..size {
            sparse.insert(i, i as u64);
            hash.insert(i, i as u64);
        }

        g.bench_with_input(BenchmarkId::new("SparseSet", size), &size, |b, _| {
            b.iter(|| {
                let mut hits = 0;
                for i in size..size * 2 {
                    if sparse.contains(&i) { hits += 1; }
                }
                black_box(hits);
            });
        });
        g.bench_with_input(BenchmarkId::new("HashMap", size), &size, |b, _| {
            b.iter(|| {
                let mut hits = 0;
                for i in size..size * 2 {
                    if hash.contains_key(&i) { hits += 1; }
                }
                black_box(hits);
            });
        });
    }
    g.finish();
}

// ── iterate ─────────────────────────────────────

fn bench_iterate(c: &mut Criterion) {
    let mut g = c.benchmark_group("iterate");
    for &size in &[100u32, 1_000, 10_000, 100_000] {
        let mut sparse: SparseSet<u32, u64> = SparseSet::with_capacity(size as usize);
        let mut hash = HashMap::with_capacity(size as usize);
        let mut vec = Vec::with_capacity(size as usize);
        for i in 0..size {
            sparse.insert(i, i as u64);
            hash.insert(i, i as u64);
            vec.push((i, i as u64));
        }

        g.bench_with_input(BenchmarkId::new("SparseSet", size), &size, |b, _| {
            b.iter(|| {
                let mut sum = 0u64;
                for (_k, &v) in sparse.iter() {
                    sum += v;
                }
                black_box(sum);
            });
        });
        g.bench_with_input(BenchmarkId::new("HashMap", size), &size, |b, _| {
            b.iter(|| {
                let mut sum = 0u64;
                for (_k, &v) in hash.iter() {
                    sum += v;
                }
                black_box(sum);
            });
        });
        g.bench_with_input(BenchmarkId::new("Vec", size), &size, |b, _| {
            b.iter(|| {
                let mut sum = 0u64;
                for (_k, v) in vec.iter() {
                    sum += v;
                }
                black_box(sum);
            });
        });
    }
    g.finish();
}

// ── remove ──────────────────────────────────────

fn bench_remove(c: &mut Criterion) {
    let mut g = c.benchmark_group("remove");
    for &size in &[100u32, 1_000, 10_000] {
        g.bench_with_input(BenchmarkId::new("SparseSet", size), &size, |b, &size| {
            b.iter(|| {
                let mut set: SparseSet<u32, u64> = SparseSet::with_capacity(size as usize);
                for i in 0..size { set.insert(i, i as u64); }
                for i in 0..size { black_box(set.remove(&i)); }
                black_box(set);
            });
        });
        g.bench_with_input(BenchmarkId::new("HashMap", size), &size, |b, &size| {
            b.iter(|| {
                let mut map = HashMap::with_capacity(size as usize);
                for i in 0..size { map.insert(i, i as u64); }
                for i in 0..size { black_box(map.remove(&i)); }
                black_box(map);
            });
        });
    }
    g.finish();
}

// ── entry（vacant → insert） ────────────────────

fn bench_entry_vacant(c: &mut Criterion) {
    let mut g = c.benchmark_group("entry_vacant");
    for &size in &[100u32, 1_000, 10_000, 100_000] {
        g.bench_with_input(BenchmarkId::new("SparseSet", size), &size, |b, &size| {
            b.iter(|| {
                let mut set: SparseSet<u32, u64> = SparseSet::with_capacity(size as usize);
                for i in 0..size {
                    set.entry(i).or_insert(i as u64);
                }
                black_box(set);
            });
        });
        g.bench_with_input(BenchmarkId::new("HashMap", size), &size, |b, &size| {
            b.iter(|| {
                let mut map = HashMap::with_capacity(size as usize);
                for i in 0..size {
                    map.entry(i).or_insert(i as u64);
                }
                black_box(map);
            });
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_insert,
    bench_get_hit,
    bench_get_miss,
    bench_iterate,
    bench_remove,
    bench_entry_vacant,
);
criterion_main!(benches);
