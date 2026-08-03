//! Criterion benchmarks for the PII scrubber hot loop.
//!
//! Run: `cargo bench -p bitvanes-core --bench scrub`
//!
//! Measures the regex sweep + anchor scan + Luhn validation at various
//! input sizes. The anchor-window scan is the new v0.3 hot path; these
//! benches quantify its cost so future SIMD work has a baseline.

use bitvanes_core::{BuiltInPattern, ScrubProfile, Scrubber};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

fn build_text(size_kb: usize) -> String {
    // Realistic-ish text: prose with periodic PII tokens.
    let unit = "Contact alice@example.com about the 4111 1111 1111 1111 card or SSN 123-45-6789. Reach out soon. ";
    let target = size_kb * 1024;
    let mut s = String::with_capacity(target);
    while s.len() < target {
        s.push_str(unit);
    }
    s
}

fn bench_scrub(c: &mut Criterion) {
    let profile = ScrubProfile {
        patterns: vec![
            BuiltInPattern::Email,
            BuiltInPattern::CreditCard,
            BuiltInPattern::Ssn,
        ],
        ..ScrubProfile::default()
    };
    let scrubber = Scrubber::from_profile(&profile).expect("compile");

    let mut group = c.benchmark_group("scrub");
    for &size in &[1, 10, 100] {
        let text = build_text(size);
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{size}KB")),
            &text,
            |b, t| {
                b.iter(|| {
                    let (out, map, findings) = scrubber.scrub(t);
                    criterion::black_box((out, map, findings));
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_scrub);
criterion_main!(benches);
