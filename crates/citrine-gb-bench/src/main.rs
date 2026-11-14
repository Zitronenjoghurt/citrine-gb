fn main() -> std::process::ExitCode {
    citrine_gb_bench::cli(std::env::args().skip(1).collect())
}
