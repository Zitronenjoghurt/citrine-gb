use crate::readings::{Reading, Readings};
use std::collections::BTreeMap;

const IR_FLOOR: f64 = 0.003;
const IR_SPREADS: f64 = 3.0;
const WALL_STEP: f64 = 0.01;
const WALL_PAIRS: usize = 5;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Verdict {
    Faster,
    Slower,
    Noise,
}

impl Verdict {
    fn word(self) -> &'static str {
        match self {
            Verdict::Faster => "faster",
            Verdict::Slower => "slower",
            Verdict::Noise => "noise",
        }
    }
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

fn quartiles(values: &[f64]) -> (f64, f64, f64) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    (at(0.25), at(0.5), at(0.75))
}

fn spread(counts: &[u64]) -> f64 {
    let min = *counts.iter().min().unwrap_or(&0) as f64;
    let max = *counts.iter().max().unwrap_or(&0) as f64;
    let values: Vec<f64> = counts.iter().map(|&c| c as f64).collect();
    let middle = median(&values);
    if middle == 0.0 {
        0.0
    } else {
        (max - min) / middle
    }
}

pub(crate) fn ir_verdict(base: &[u64], head: &[u64]) -> (f64, f64, Verdict) {
    let to_f = |counts: &[u64]| counts.iter().map(|&c| c as f64).collect::<Vec<_>>();
    let ratio = median(&to_f(head)) / median(&to_f(base));
    let bound = (IR_SPREADS * spread(base).max(spread(head))).max(IR_FLOOR);
    let verdict = if ratio < 1.0 - bound {
        Verdict::Faster
    } else if ratio > 1.0 + bound {
        Verdict::Slower
    } else {
        Verdict::Noise
    };
    (ratio - 1.0, bound, verdict)
}

pub(crate) fn wall_verdict(base: &[f64], head: &[f64]) -> Option<(f64, Verdict)> {
    if base.len() != head.len() || base.len() < WALL_PAIRS {
        return None;
    }
    let ratios: Vec<f64> = base.iter().zip(head).map(|(b, h)| h / b).collect();
    let (q1, middle, q3) = quartiles(&ratios);
    let verdict = if q3 < 1.0 && middle < 1.0 - WALL_STEP {
        Verdict::Faster
    } else if q1 > 1.0 && middle > 1.0 + WALL_STEP {
        Verdict::Slower
    } else {
        Verdict::Noise
    };
    Some((middle - 1.0, verdict))
}

fn percent(delta: f64) -> String {
    format!("{:+.2}%", delta * 100.0)
}

pub(crate) fn report(base: &Readings, head: &Readings, profile: &str) -> (String, bool) {
    let mut out = String::new();
    let same_host =
        base.meta.contains_key("host") && base.meta.get("host") == head.meta.get("host");
    out.push_str(&format!(
        "base {} ({}), head {} ({}), Ir profile {profile}\n\n",
        base.meta.get("commit").map_or("?", String::as_str),
        base.meta.get("version").map_or("?", String::as_str),
        head.meta.get("commit").map_or("?", String::as_str),
        head.meta.get("version").map_or("?", String::as_str),
    ));
    out.push_str("| workload | console | Ir delta | bound | Ir | wall delta | wall |\n|---|---|---|---|---|---|---|\n");
    let mut per_console: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let empty = Reading::default();
    for (id, now) in &head.workloads {
        let before = base.workloads.get(id).unwrap_or(&empty);
        let ir = match (before.ir.get(profile), now.ir.get(profile)) {
            (Some(b), Some(h)) if !b.is_empty() && !h.is_empty() => Some(ir_verdict(b, h)),
            _ => None,
        };
        let wall = if same_host {
            wall_verdict(&before.wall_ms, &now.wall_ms)
        } else {
            None
        };
        if let Some((delta, _, _)) = ir {
            per_console
                .entry(now.console.clone())
                .or_default()
                .push(1.0 + delta);
        }
        out.push_str(&format!(
            "| {id} | {} | {} | {} | {} | {} | {} |\n",
            now.console,
            ir.map_or("-".into(), |(d, _, _)| percent(d)),
            ir.map_or("-".into(), |(_, b, _)| format!("{:.2}%", b * 100.0)),
            if now.failures.is_empty() {
                ir.map_or("-", |(_, _, v)| v.word())
            } else {
                "FAIL"
            },
            wall.map_or("-".into(), |(d, _)| percent(d)),
            wall.map_or(if same_host { "-" } else { "other host" }, |(_, v)| v
                .word()),
        ));
    }
    out.push_str("\n| console | Ir geometric mean delta | workloads |\n|---|---|---|\n");
    for (console, ratios) in &per_console {
        let mean = (ratios.iter().map(|r| r.ln()).sum::<f64>() / ratios.len() as f64).exp();
        out.push_str(&format!(
            "| {console} | {} | {} |\n",
            percent(mean - 1.0),
            ratios.len()
        ));
    }
    let mut sizes = String::new();
    for (name, bytes) in &head.sizes {
        let before = base.sizes.get(name);
        sizes.push_str(&format!(
            "| {name} | {} | {bytes} | {} |\n",
            before.map_or("-".into(), u64::to_string),
            before.map_or("-".into(), |b| percent(*bytes as f64 / *b as f64 - 1.0)),
        ));
    }
    if !sizes.is_empty() {
        out.push_str("\n| size | base bytes | head bytes | delta |\n|---|---|---|---|\n");
        out.push_str(&sizes);
    }
    let failed = head.failed();
    for (id, failures) in &failed {
        out.push_str(&format!("\nFAIL {id}: {}\n", failures.join("; ")));
    }
    (out, failed.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_inside_the_noise_bound_is_noise() {
        let (_, bound, verdict) = ir_verdict(&[1000, 1001, 1000], &[1002, 1002, 1003]);
        assert_eq!(bound, IR_FLOOR);
        assert_eq!(verdict, Verdict::Noise);
    }

    #[test]
    fn a_change_past_three_spreads_counts() {
        let (delta, _, verdict) = ir_verdict(&[1000, 1000, 1000], &[900, 900, 901]);
        assert!(delta < -0.09);
        assert_eq!(verdict, Verdict::Faster);
        let (_, bound, verdict) = ir_verdict(&[1000, 1100, 1000], &[1200, 1200, 1200]);
        assert!((bound - 0.3).abs() < 1e-9);
        assert_eq!(verdict, Verdict::Noise);
    }

    #[test]
    fn wall_time_needs_most_pairs_on_one_side() {
        let base = [10.0; 6];
        assert_eq!(
            wall_verdict(&base, &[9.0, 9.1, 9.2, 9.0, 9.1, 11.0])
                .unwrap()
                .1,
            Verdict::Faster
        );
        assert_eq!(
            wall_verdict(&base, &[9.0, 11.0, 9.0, 11.0, 10.0, 10.0])
                .unwrap()
                .1,
            Verdict::Noise
        );
        assert!(wall_verdict(&base[..3], &base[..3]).is_none());
    }
}
