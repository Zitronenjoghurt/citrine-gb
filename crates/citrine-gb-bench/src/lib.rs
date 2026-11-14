mod callgrind;
mod cli;
mod compare;
mod digest;
mod ds;
mod flame;
mod input;
mod manifest;
mod readings;
mod roms;
mod run;

pub fn cli(args: Vec<String>) -> std::process::ExitCode {
    cli::main(args)
}
