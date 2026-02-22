use crate::gb::cpu::Cpu;
use crate::tests::TestBus;
use std::path::PathBuf;

#[test]
pub fn run() {
    let tests_dir = PathBuf::from("../tests/cpu-adtennant/v2");
    let whitelist: &[&str] = &[];
    let mut total = 0;
    let mut total_failed = 0;
    let mut failed_opcodes: Vec<String> = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(&tests_dir)
        .expect("test dir not found")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .filter(|e| {
            whitelist.is_empty()
                || whitelist.iter().any(|w| {
                    e.path()
                        .file_stem()
                        .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(w))
                })
        })
        .collect();
    entries.sort_by_key(|e| e.path());

    for entry in entries {
        let path = entry.path();
        let data = std::fs::read_to_string(&path).unwrap();
        let tests: Vec<TestCase> = serde_json::from_str(&data).unwrap();
        let count = tests.len();
        let fails = tests
            .iter()
            .filter(|t| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_test(t))).is_err()
            })
            .count();

        total += count;
        total_failed += fails;

        if fails > 0 {
            failed_opcodes.push(format!(
                "  {} {}/{}",
                path.file_name().unwrap().to_string_lossy(),
                fails,
                count,
            ));
        }
    }

    if total_failed > 0 {
        panic!(
            "\n{}/{} tests failed across {} opcodes:\n{}\n",
            total_failed,
            total,
            failed_opcodes.len(),
            failed_opcodes.join("\n"),
        );
    }
}

fn run_test(test: &TestCase) {
    let mut cpu = Cpu::from(&test.initial);
    let mut bus = TestBus::from(&test.initial);

    cpu.ir = *bus.data.get(&test.initial.pc.wrapping_sub(1)).unwrap();
    cpu.step(&mut bus);

    // adtennant's cases dont include the final IR
    let mut expected_cpu = Cpu::from(&test.expected);
    expected_cpu.ir = cpu.ir;
    expected_cpu.ime = cpu.ime;

    assert_eq!(cpu, expected_cpu, "CPU | {}", test.name);
    assert_eq!(
        bus,
        TestBus::from((&test.expected, test)),
        "RAM | {}",
        test.name
    );
}

#[derive(Debug, serde::Deserialize)]
struct TestCase {
    name: String,
    initial: TestState,
    #[serde(alias = "final")]
    expected: TestState,
    #[serde(default, deserialize_with = "deserialize_cycles")]
    cycles: Vec<BusCycle>,
}

#[derive(Debug, serde::Deserialize)]
struct BusCycle(u16, u8, String);

#[derive(Debug, serde::Deserialize)]
struct TestState {
    a: u8,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    f: u8,
    h: u8,
    l: u8,
    pc: u16,
    sp: u16,
    ram: Vec<[u16; 2]>,
}

impl From<&TestState> for Cpu {
    fn from(s: &TestState) -> Self {
        Self {
            a: s.a,
            b: s.b,
            c: s.c,
            d: s.d,
            e: s.e,
            f: s.f.into(),
            h: s.h,
            l: s.l,
            sp: s.sp,
            pc: s.pc,
            ir: 0,
            ime: false,
            ime_next: false,
            halted: false,
        }
    }
}

impl From<&TestState> for TestBus {
    fn from(s: &TestState) -> Self {
        Self {
            data: s.ram.iter().map(|e| (e[0], e[1] as u8)).collect(),
            history: Vec::new(),
        }
    }
}

impl From<(&TestState, &TestCase)> for TestBus {
    fn from(s: (&TestState, &TestCase)) -> Self {
        Self {
            data: s.0.ram.iter().map(|e| (e[0], e[1] as u8)).collect(),
            history: s.1.cycles.iter().map(|e| (e.0, e.1, e.2.clone())).collect(),
        }
    }
}

fn deserialize_cycles<'de, D>(deserializer: D) -> Result<Vec<BusCycle>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    Option::<Vec<Option<BusCycle>>>::deserialize(deserializer)
        .map(|o| o.unwrap_or_default().into_iter().flatten().collect())
}
