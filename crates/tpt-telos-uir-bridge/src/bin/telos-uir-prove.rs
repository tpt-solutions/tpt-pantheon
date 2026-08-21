//! `telos-uir-prove` binary — a thin entry point over the prover-bridge CLI
//! logic in `tpt_telos_uir_bridge::run_cli`.

fn main() {
    std::process::exit(tpt_telos_uir_bridge::run_cli(std::env::args()) as i32);
}
