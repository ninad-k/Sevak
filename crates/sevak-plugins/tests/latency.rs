//! A coarse guard against catastrophic search regressions: the real default
//! engine over a large mock index must answer each typical query in well under
//! a tenth of a second.
//!
//! The engine itself warns above 16 ms (`LATENCY_BUDGET`); this test allows 100 ms
//! at the 95th percentile, so a slow or noisy CI runner does not make it flaky
//! while an accidental O(n^2) or a per-query disk read still fails it. The tighter
//! numbers are tracked by `cargo bench -p sevak-plugins` (see docs/testing.md).
//!
//! It only means something in an optimized build, so it skips itself in debug
//! builds; CI runs it with `cargo test --release -p sevak-plugins --test latency`.

use std::time::{Duration, Instant};

#[path = "../benches/fixture/mod.rs"]
mod fixture;

use fixture::{Fixture, Sizes, QUERIES};

/// p95 each query must stay under.
const BUDGET: Duration = Duration::from_millis(100);
const WARM_UP: usize = 5;
const SAMPLES: usize = 40;
const ATTEMPTS: usize = 3;

fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    sorted[(sorted.len() * percent / 100).min(sorted.len() - 1)]
}

#[test]
fn engine_query_latency_budget() {
    if cfg!(debug_assertions) {
        eprintln!("engine_query_latency_budget: skipped in a debug build (run with --release)");
        return;
    }

    let fixture = Fixture::ready(Sizes::LARGE);
    let mut over = Vec::new();
    println!(
        "{:<22} {:>9} {:>9} {:>9}  results",
        "query", "p50", "p95", "max"
    );
    for (name, input, answers) in QUERIES {
        // The fixture must really answer, or a fast empty result would pass.
        let results = fixture.engine.query(input);
        assert!(
            !*answers || !results.is_empty(),
            "no results for {input:?} ({name}); the fixture is broken"
        );
        for _ in 0..WARM_UP {
            fixture.engine.query(input);
        }
        // A busy CI machine can stall a whole round of samples, so a query gets
        // up to ATTEMPTS rounds and passes with the first whose p95 is in budget.
        // A real regression is slow in every round.
        let mut best: Option<(Duration, Duration, Duration)> = None;
        for _ in 0..ATTEMPTS {
            let mut times: Vec<Duration> = (0..SAMPLES)
                .map(|_| {
                    let started = Instant::now();
                    std::hint::black_box(fixture.engine.query(input));
                    started.elapsed()
                })
                .collect();
            times.sort();
            let round = (
                percentile(&times, 50),
                percentile(&times, 95),
                *times.last().unwrap(),
            );
            if best.is_none_or(|(_, p95, _)| round.1 < p95) {
                best = Some(round);
            }
            if round.1 < BUDGET {
                break;
            }
        }
        let (p50, p95, max) = best.unwrap();
        println!(
            "{name:<22} {:>7.2}ms {:>7.2}ms {:>7.2}ms  {}",
            p50.as_secs_f64() * 1000.0,
            p95.as_secs_f64() * 1000.0,
            max.as_secs_f64() * 1000.0,
            results.len()
        );
        if p95 >= BUDGET {
            over.push(format!("{name} ({input:?}): p95 {p95:?}"));
        }
    }
    assert!(
        over.is_empty(),
        "queries over the {BUDGET:?} budget at p95:\n  {}",
        over.join("\n  ")
    );
}
