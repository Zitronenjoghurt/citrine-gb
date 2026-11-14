use citrine_gb::console::Keys;

pub struct Step {
    from: usize,
    to: usize,
    keys: Keys,
}

fn key(name: &str) -> Keys {
    match name {
        "A" => Keys::A,
        "B" => Keys::B,
        "X" => Keys::X,
        "Y" => Keys::Y,
        "L" => Keys::L,
        "R" => Keys::R,
        "ST" => Keys::START,
        "SE" => Keys::SELECT,
        "UP" => Keys::UP,
        "DN" => Keys::DOWN,
        "LT" => Keys::LEFT,
        "RT" => Keys::RIGHT,
        _ => panic!("unknown key {name}"),
    }
}

pub fn parse(script: &str) -> Vec<Step> {
    script
        .split(',')
        .filter(|token| !token.is_empty())
        .map(|token| {
            let (range, keys) = token.split_once(':').expect("from-to:KEYS");
            let (from, to) = range.split_once('-').unwrap_or((range, range));
            Step {
                from: from.parse().expect("a start frame"),
                to: to.parse().expect("an end frame"),
                keys: keys
                    .split('+')
                    .map(key)
                    .fold(Keys::empty(), |all, k| all | k),
            }
        })
        .collect()
}

pub fn keys_at(steps: &[Step], frame: usize) -> Keys {
    steps
        .iter()
        .filter(|step| (step.from..=step.to).contains(&frame))
        .fold(Keys::empty(), |all, step| all | step.keys)
}
