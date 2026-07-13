fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--version") {
        println!("goldeneye {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    goldeneye::run_session(std::io::stdin().lock(), std::io::stdout().lock())
}
