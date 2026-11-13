mod cli;
mod clipboard;
mod draw;
mod geom;
mod look;
mod wayland;

use std::process;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match cli::parse_args(&args) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("infynitae-shot: {err}");
            process::exit(2);
        }
    };

    let (mode, live, file) = match cmd {
        cli::Command::Help => {
            println!("{}", cli::help_text());
            process::exit(0);
        }
        cli::Command::Run { mode, live, file } => (mode, live, file),
    };

    let look = look::Look::load();
    let png_bytes = match wayland::run_wayland(mode, live, look) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => process::exit(1),
        Err(err) => {
            eprintln!("infynitae-shot: {err}");
            process::exit(1);
        }
    };

    if let Err(err) = geom::write_png_file(&file, &png_bytes) {
        eprintln!("infynitae-shot: {err}");
        process::exit(1);
    }

    clipboard::publish_and_fork_clipboard(&file, png_bytes);
}
