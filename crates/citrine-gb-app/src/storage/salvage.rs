use serde::de::DeserializeOwned;
use serde_json::Value;
use serde_path_to_error::Segment;

const MOST_FIXES: usize = 10_000;

pub struct Salvaged<T> {
    pub value: T,
    pub dropped: Vec<String>,
}

pub fn salvage<T: DeserializeOwned>(
    mut stored: Value,
    fallback: Option<&Value>,
) -> Option<Salvaged<T>> {
    let mut dropped = Vec::new();
    for _ in 0..MOST_FIXES {
        let err = match serde_path_to_error::deserialize::<_, T>(&stored) {
            Ok(value) => return Some(Salvaged { value, dropped }),
            Err(err) => err,
        };
        let steps = reach(&stored, err.path());
        if steps.is_empty() {
            return None;
        }
        let before = fallback
            .filter(|_| steps.iter().all(|step| matches!(step, Step::Key(_))))
            .and_then(|fallback| node(fallback, &steps))
            .filter(|mine| node(&stored, &steps) != Some(*mine))
            .cloned();
        place(&mut stored, &steps, before)?;
        dropped.push(named(&steps));
    }
    None
}

pub fn parse(raw: &[u8]) -> Value {
    if let Ok(value) = serde_json::from_slice(raw) {
        return value;
    }
    let text = String::from_utf8_lossy(raw);
    let mut end = text.len();
    while let Some(cut) = text[..end].rfind("\n  \"") {
        let head = text[..cut].trim_end().trim_end_matches(',');
        if let Ok(value) = serde_json::from_str(&format!("{head}\n}}")) {
            return value;
        }
        end = cut;
    }
    Value::Null
}

enum Step {
    Key(String),
    Item(usize),
}

fn reach(root: &Value, path: &serde_path_to_error::Path) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut at = root;
    for segment in path {
        let next = match (at, segment) {
            (Value::Object(fields), Segment::Map { key } | Segment::Enum { variant: key }) => {
                fields.get(key).map(|next| (next, Step::Key(key.clone())))
            }
            (Value::Array(items), Segment::Seq { index }) => {
                items.get(*index).map(|next| (next, Step::Item(*index)))
            }
            _ => None,
        };
        let Some((next, step)) = next else {
            break;
        };
        steps.push(step);
        at = next;
    }
    steps
}

fn node<'a>(root: &'a Value, steps: &[Step]) -> Option<&'a Value> {
    steps.iter().try_fold(root, |at, step| match step {
        Step::Key(key) => at.get(key),
        Step::Item(index) => at.get(index),
    })
}

fn place(root: &mut Value, steps: &[Step], value: Option<Value>) -> Option<()> {
    let (last, parents) = steps.split_last()?;
    let mut at = root;
    for step in parents {
        at = match step {
            Step::Key(key) => at.get_mut(key)?,
            Step::Item(index) => at.get_mut(index)?,
        };
    }
    match (last, value) {
        (Step::Key(key), Some(value)) => {
            at.as_object_mut()?.insert(key.clone(), value);
        }
        (Step::Key(key), None) => {
            at.as_object_mut()?.remove(key);
        }
        (Step::Item(index), Some(value)) => *at.get_mut(index)? = value,
        (Step::Item(index), None) => {
            at.as_array_mut()?.remove(*index);
        }
    }
    Some(())
}

fn named(steps: &[Step]) -> String {
    let mut name = String::new();
    for step in steps {
        match step {
            Step::Key(key) if name.is_empty() => name.push_str(key),
            Step::Key(key) => {
                name.push('.');
                name.push_str(key);
            }
            Step::Item(index) => name.push_str(&format!("[{index}]")),
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::{Salvaged, parse, salvage};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    #[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(default)]
    struct Inner {
        size: u8,
        mode: Mode,
    }

    #[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    enum Mode {
        #[default]
        Plain,
        Fancy,
    }

    #[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(default)]
    struct Outer {
        volume: f32,
        inner: Inner,
        names: BTreeMap<String, Inner>,
        keys: Vec<Mode>,
        lanes: [bool; 2],
    }

    fn load(stored: serde_json::Value) -> Salvaged<Outer> {
        salvage(stored, None).expect("an object always loads")
    }

    #[test]
    fn a_whole_file_loads_untouched() {
        let loaded = load(json!({ "volume": 0.5, "inner": { "size": 3, "mode": "Fancy" } }));
        assert!(loaded.dropped.is_empty());
        assert_eq!(loaded.value.inner.mode, Mode::Fancy);
    }

    #[test]
    fn one_bad_field_falls_back_alone() {
        let loaded = load(json!({
            "volume": 0.5,
            "inner": { "size": 3, "mode": "Gone" },
            "lanes": [true, true],
        }));
        assert_eq!(loaded.dropped, ["inner.mode"]);
        assert_eq!(loaded.value.volume, 0.5);
        assert_eq!(loaded.value.inner.size, 3);
        assert_eq!(loaded.value.inner.mode, Mode::Plain);
        assert_eq!(loaded.value.lanes, [true, true]);
    }

    #[test]
    fn a_bad_map_entry_or_list_item_goes_alone() {
        let loaded = load(json!({
            "names": {
                "a": { "size": 1, "mode": "Fancy" },
                "b": { "size": "big" },
            },
            "keys": ["Fancy", "Gone", "Plain"],
        }));
        assert_eq!(loaded.dropped, ["keys[1]", "names.b.size"]);
        assert_eq!(loaded.value.names["a"].mode, Mode::Fancy);
        assert_eq!(loaded.value.names["b"], Inner::default());
        assert_eq!(loaded.value.keys, [Mode::Fancy, Mode::Plain]);
    }

    #[test]
    fn a_fixed_length_list_that_changed_falls_back_whole() {
        let loaded = load(json!({ "volume": 0.25, "lanes": [true, true, true] }));
        assert_eq!(loaded.dropped, ["lanes"]);
        assert_eq!(loaded.value.volume, 0.25);
        assert_eq!(loaded.value.lanes, [false, false]);
    }

    #[test]
    fn a_file_that_is_not_an_object_loads_nothing() {
        assert!(salvage::<Outer>(json!("a string"), None).is_none());
    }

    #[derive(Debug, serde::Deserialize)]
    #[serde(default)]
    struct Upgraded {
        volume: f32,
        #[serde(default)]
        fresh: bool,
    }

    impl Default for Upgraded {
        fn default() -> Self {
            Self {
                volume: 1.0,
                fresh: true,
            }
        }
    }

    #[test]
    fn a_field_missing_from_an_old_file_reads_as_a_strict_load_reads_it() {
        let loaded = salvage::<Upgraded>(json!({ "volume": null }), None).unwrap();
        assert_eq!(loaded.dropped, ["volume"]);
        assert_eq!(loaded.value.volume, 1.0);
        assert!(!loaded.value.fresh);
    }

    #[test]
    fn a_bad_field_keeps_the_fallback_value() {
        let mine = serde_json::to_value(Outer {
            volume: 0.75,
            ..Outer::default()
        })
        .unwrap();
        let mut merged = mine.as_object().unwrap().clone();
        merged.extend(
            json!({ "volume": "loud", "inner": { "size": 9, "mode": "Fancy" } })
                .as_object()
                .unwrap()
                .clone(),
        );
        let kept = salvage::<Outer>(Value::Object(merged), Some(&mine)).unwrap();
        assert_eq!(kept.dropped, ["volume"]);
        assert_eq!(kept.value.volume, 0.75);
        assert_eq!(kept.value.inner.mode, Mode::Fancy);
    }

    #[test]
    fn a_cut_off_file_keeps_the_fields_before_the_cut() {
        let whole = serde_json::to_string_pretty(&Outer {
            volume: 0.5,
            keys: vec![Mode::Fancy],
            lanes: [true, false],
            ..Outer::default()
        })
        .unwrap();
        let cut = &whole[..whole.find("\"lanes\"").unwrap() + 12];
        let loaded = load(parse(cut.as_bytes()));
        assert_eq!(loaded.value.volume, 0.5);
        assert_eq!(loaded.value.keys, [Mode::Fancy]);
        assert_eq!(loaded.value.lanes, [false, false]);
        assert_eq!(parse(b"not json"), serde_json::Value::Null);
    }
}
