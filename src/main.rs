fn main() {
    if let Err(e) = grove::cli::main() {
        eprintln!("grove: {e:#}");
        std::process::exit(1);
    }
}
