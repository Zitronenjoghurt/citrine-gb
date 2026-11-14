use std::collections::{BTreeMap, HashMap};

const ROOT: &str = "citrine_gb_bench::run::measured";
const MAX_DEPTH: usize = 160;

#[derive(Default)]
pub(crate) struct Profile {
    pub(crate) total: u64,
    own: HashMap<String, u64>,
    calls: HashMap<String, BTreeMap<String, u64>>,
}

fn clean(name: &str) -> String {
    let name = match name.rfind('\'') {
        Some(at) if name[at + 1..].chars().all(|c| c.is_ascii_digit()) => &name[..at],
        _ => name,
    };
    name.replace(';', ",")
}

struct Names(HashMap<String, String>);

impl Names {
    fn resolve(&mut self, raw: &str) -> String {
        let raw = raw.trim();
        if let Some(rest) = raw.strip_prefix('(')
            && let Some((id, name)) = rest.split_once(')')
        {
            let name = name.trim();
            if name.is_empty() {
                return self.0.get(id).cloned().unwrap_or_default();
            }
            let name = clean(name);
            self.0.insert(id.to_string(), name.clone());
            return name;
        }
        clean(raw)
    }
}

impl Profile {
    pub(crate) fn parse(text: &str) -> Self {
        let mut profile = Profile::default();
        let mut names = Names(HashMap::new());
        let mut current = String::new();
        let mut callee: Option<String> = None;
        let mut pending_call = false;
        let mut summary = None;
        for line in text.lines() {
            let Some(first) = line.chars().next() else {
                continue;
            };
            if first.is_ascii_digit() || matches!(first, '+' | '-' | '*') {
                let cost: u64 = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|cost| cost.parse().ok())
                    .unwrap_or(0);
                if pending_call {
                    if let Some(callee) = &callee {
                        *profile
                            .calls
                            .entry(current.clone())
                            .or_default()
                            .entry(callee.clone())
                            .or_default() += cost;
                    }
                    pending_call = false;
                } else {
                    *profile.own.entry(current.clone()).or_default() += cost;
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("fn=") {
                current = names.resolve(rest);
            } else if let Some(rest) = line.strip_prefix("cfn=") {
                callee = Some(names.resolve(rest));
            } else if line.starts_with("calls=") {
                pending_call = true;
            } else if let Some(rest) = line
                .strip_prefix("summary:")
                .or_else(|| line.strip_prefix("totals:"))
            {
                summary = rest.split_whitespace().next().and_then(|n| n.parse().ok());
            }
        }
        profile.total = summary.unwrap_or_else(|| profile.own.values().sum());
        profile
    }

    fn inclusive(&self, name: &str) -> u64 {
        let own = self.own.get(name).copied().unwrap_or(0);
        let calls: u64 = self
            .calls
            .get(name)
            .map(|calls| {
                calls
                    .iter()
                    .filter(|(callee, _)| callee.as_str() != name)
                    .map(|(_, cost)| cost)
                    .sum()
            })
            .unwrap_or(0);
        own + calls
    }

    fn roots(&self) -> Vec<String> {
        let mut named: Vec<String> = self
            .own
            .keys()
            .chain(self.calls.keys())
            .filter(|name| name.starts_with(ROOT))
            .cloned()
            .collect();
        named.sort();
        named.dedup();
        if !named.is_empty() {
            return named;
        }
        let called: std::collections::HashSet<&String> = self
            .calls
            .iter()
            .flat_map(|(caller, callees)| callees.keys().filter(move |callee| *callee != caller))
            .collect();
        let mut roots: Vec<String> = self
            .own
            .keys()
            .chain(self.calls.keys())
            .filter(|name| !called.contains(name))
            .cloned()
            .collect();
        roots.sort();
        roots.dedup();
        roots
    }

    pub(crate) fn folded(&self, smallest: f64) -> Vec<String> {
        let floor = (self.total as f64 * smallest) as u64;
        let mut out = BTreeMap::new();
        for root in self.roots() {
            let weight = self.inclusive(&root);
            let mut stack = vec![root.clone()];
            self.expand(&root, weight, floor, &mut stack, &mut out);
        }
        out.into_iter()
            .map(|(stack, count)| format!("{stack} {count}"))
            .collect()
    }

    fn expand(
        &self,
        name: &str,
        weight: u64,
        floor: u64,
        stack: &mut Vec<String>,
        out: &mut BTreeMap<String, u64>,
    ) {
        let inclusive = self.inclusive(name);
        if inclusive == 0 || weight == 0 {
            return;
        }
        let scale =
            |cost: u64| (u128::from(cost) * u128::from(weight) / u128::from(inclusive)) as u64;
        let mut own = scale(self.own.get(name).copied().unwrap_or(0));
        if let Some(calls) = self.calls.get(name) {
            for (callee, cost) in calls {
                if callee == name {
                    continue;
                }
                let share = scale(*cost);
                if share < floor || stack.len() >= MAX_DEPTH || stack.contains(callee) {
                    own += share;
                    continue;
                }
                stack.push(callee.clone());
                self.expand(callee, share, floor, stack, out);
                stack.pop();
            }
        }
        if own > 0 {
            *out.entry(stack.join(";")).or_default() += own;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "events: Ir\nfn=citrine_gb_bench::run::measured<x>\n1 10\ncfn=a\ncalls=2 5\n5 90\nfn=a\n5 30\ncfn=b\ncalls=1 7\n7 60\nfn=b\n7 60\ntotals: 100\n";

    #[test]
    fn totals_come_from_the_summary_line() {
        assert_eq!(Profile::parse(SAMPLE).total, 100);
    }

    #[test]
    fn stacks_split_inclusive_costs_by_call() {
        let folded = Profile::parse(SAMPLE).folded(0.0);
        assert_eq!(
            folded,
            vec![
                "citrine_gb_bench::run::measured<x> 10",
                "citrine_gb_bench::run::measured<x>;a 30",
                "citrine_gb_bench::run::measured<x>;a;b 60",
            ]
        );
    }

    #[test]
    fn compressed_names_resolve_by_id() {
        let text = "fn=(1) citrine_gb_bench::run::measured\n1 4\ncfn=(2) c'2\ncalls=1 1\n1 6\nfn=(2)\n1 6\n";
        let profile = Profile::parse(text);
        assert_eq!(profile.total, 10);
        assert!(
            profile
                .folded(0.0)
                .iter()
                .any(|line| line.ends_with(";c 6"))
        );
    }
}
