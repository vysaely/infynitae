//! `infynitae-files`: shared binary for Infynitae's portal file picker (`--portal`, stage 1)
//! and file manager (stage 2, `docs/FILE_MANAGER.md`).

mod draw;
mod fs_list;
mod look;
mod mime;
mod places;
mod portal;
mod preview;
mod recent;
mod text_input;
mod ui;
mod wayland;

use portal::protocol::PortalRequest;
use std::io::{self, Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "Usage:\n  infynitae-files --portal    Read one FileChooser JSON request from stdin and write the reply to stdout"
        );
        return ExitCode::SUCCESS;
    }

    if args.first().map(String::as_str) != Some("--portal") {
        eprintln!(
            "infynitae-files: standalone file manager mode is planned for stage 2; pass --portal for FileChooser mode"
        );
        return ExitCode::from(2);
    }

    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!("infynitae-files: failed to read stdin: {e}");
        return ExitCode::FAILURE;
    }

    let req: PortalRequest = match serde_json::from_str(&input) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("infynitae-files: invalid portal request JSON: {e}");
            return ExitCode::FAILURE;
        }
    };

    match wayland::run_portal_picker(req) {
        Ok(resp) => match serde_json::to_string(&resp) {
            Ok(line) => {
                let mut stdout = io::stdout().lock();
                if let Err(e) = writeln!(stdout, "{line}") {
                    eprintln!("infynitae-files: failed to write reply: {e}");
                    return ExitCode::FAILURE;
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("infynitae-files: failed to serialize reply: {e}");
                ExitCode::FAILURE
            }
        },
        Err(err) => {
            eprintln!("infynitae-files: {err}");
            ExitCode::FAILURE
        }
    }
}
