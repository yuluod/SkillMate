fn main() {
    if let Err(error) = skillmate_lib::cli::run(std::env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
