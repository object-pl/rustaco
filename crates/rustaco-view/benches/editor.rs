//! Benchmarks for the paths a user feels: loading, typing, scrolling, laying out a frame.
//! Run with `cargo bench -p rustaco-view`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use rustaco_core::{Document, Selection, Selections};
use rustaco_view::{Metrics, Plain, View};

const LINES: usize = 1_000_000;

fn big_text() -> String {
    (0..LINES).map(|i| format!("let value_{i} = compute({i}, \"some text here\");\n")).collect()
}

fn view() -> View {
    View::new(Metrics { char_width: 8.0, line_height: 20.0 }, 1200.0, 900.0)
}

fn benches(c: &mut Criterion) {
    let text = big_text();

    c.bench_function("load 1M lines", |b| b.iter(|| Document::new(black_box(&text))));

    let base = Document::new(&text);
    let middle = base.rope().line_to_char(LINES / 2);

    c.bench_function("keystroke in the middle of 1M lines", |b| {
        b.iter_batched_ref(
            || {
                let mut doc = base.clone();
                doc.set_selections(Selections::single(Selection::cursor(middle)));
                doc
            },
            |doc| doc.insert("x"),
            BatchSize::SmallInput,
        )
    });

    c.bench_function("keystroke with 100 cursors", |b| {
        b.iter_batched_ref(
            || {
                let mut doc = base.clone();
                let cursors = (0..100).map(|i| Selection::cursor(doc.rope().line_to_char(i * 1000))).collect();
                doc.set_selections(Selections::new(cursors, 0));
                doc
            },
            |doc| doc.insert("x"),
            BatchSize::SmallInput,
        )
    });

    c.bench_function("layout a frame at the middle of 1M lines", |b| {
        let mut v = view();
        v.scroll_to(base.rope(), v.line_top(LINES / 2), 0.0);
        b.iter(|| v.layout(black_box(&base), &Plain))
    });

    c.bench_function("hit test", |b| {
        let mut v = view();
        v.scroll_to(base.rope(), v.line_top(LINES / 2), 0.0);
        b.iter(|| v.hit_test(base.rope(), black_box(300.0), black_box(450.0)))
    });
}

criterion_group! {
    name = editor;
    config = Criterion::default().sample_size(30);
    targets = benches
}
criterion_main!(editor);
