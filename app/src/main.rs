fn main() {
    env_logger::init();
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    if let Err(e) = m360_ui::run(open) {
        eprintln!("Merge360 failed to start: {e}");
        std::process::exit(1);
    }
}
