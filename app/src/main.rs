use std::process::Command;

const INSTALL_SH: &str =
    "https://raw.githubusercontent.com/Volko61/Merge360-Vibecoded-Inspired-Fusion360/main/install.sh";

/// Update through the channel Merge360 was installed with.
fn update() -> i32 {
    let status = if cfg!(windows) {
        Command::new("winget")
            .args(["upgrade", "--id", "Volko61.Merge360", "--exact"])
            .status()
    } else {
        Command::new("sh")
            .args(["-c", &format!("curl -fsSL {INSTALL_SH} | sh")])
            .status()
    };
    match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("Update failed: {e}");
            1
        }
    }
}

fn main() {
    let arg = std::env::args_os().nth(1);
    match arg.as_deref().and_then(|a| a.to_str()) {
        Some("--version" | "-V") => {
            println!("merge360 {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Some("--update") => std::process::exit(update()),
        _ => {}
    }
    env_logger::init();
    let open = arg.map(std::path::PathBuf::from);
    if let Err(e) = m360_ui::run(open) {
        eprintln!("Merge360 failed to start: {e}");
        std::process::exit(1);
    }
}
