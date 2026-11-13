use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Screen,
    Region,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Run {
        mode: Mode,
        live: bool,
        file: PathBuf,
    },
}

pub fn help_text() -> &'static str {
    "Usage:
  infynitae-shot screen FILE
  infynitae-shot region [--live] FILE
  infynitae-shot window [--live] FILE
  infynitae-shot --help

Modes:
  screen   Capture the output under the pointer (or the first output) to FILE
  region   Freeze outputs, pick a rectangle or window, and save the cut to FILE
  window   Freeze outputs, pick a window, and save it to FILE

Options:
  --live   Do not capture before showing the overlay; capture after picking
  -h, --help  Show this help message"
}

/// Parses command-line arguments (excluding `argv[0]`). Returns a one-line message on error.
pub fn parse_args<I, S>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let raw: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    if raw.is_empty() {
        return Err("missing mode (expected screen, region, window, or --help)".to_string());
    }
    if raw.len() == 1 && matches!(raw[0].as_str(), "-h" | "--help") {
        return Ok(Command::Help);
    }

    let mode = match raw[0].as_str() {
        "screen" => Mode::Screen,
        "region" => Mode::Region,
        "window" => Mode::Window,
        other => {
            if other.starts_with('-') {
                return Err(format!("unknown option: {other}"));
            }
            return Err(format!("unknown mode: {other}"));
        }
    };

    let mut live = false;
    let mut file: Option<PathBuf> = None;

    for arg in &raw[1..] {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--live" => {
                if mode == Mode::Screen {
                    return Err("--live is only valid for region or window".to_string());
                }
                live = true;
            }
            s if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            s => {
                if file.is_some() {
                    return Err(format!("unexpected extra argument: {s}"));
                }
                file = Some(PathBuf::from(s));
            }
        }
    }

    let Some(file) = file else {
        return Err("missing output file path".to_string());
    };

    Ok(Command::Run { mode, live, file })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_flag_returns_help() {
        assert_eq!(parse_args(["--help"]).unwrap(), Command::Help);
        assert_eq!(parse_args(["-h"]).unwrap(), Command::Help);
        assert_eq!(parse_args(["region", "--help"]).unwrap(), Command::Help);
    }

    #[test]
    fn parses_all_modes_and_live() {
        assert_eq!(
            parse_args(["screen", "/tmp/a.png"]).unwrap(),
            Command::Run {
                mode: Mode::Screen,
                live: false,
                file: PathBuf::from("/tmp/a.png"),
            }
        );
        assert_eq!(
            parse_args(["region", "/tmp/b.png"]).unwrap(),
            Command::Run {
                mode: Mode::Region,
                live: false,
                file: PathBuf::from("/tmp/b.png"),
            }
        );
        assert_eq!(
            parse_args(["region", "--live", "/tmp/b.png"]).unwrap(),
            Command::Run {
                mode: Mode::Region,
                live: true,
                file: PathBuf::from("/tmp/b.png"),
            }
        );
        assert_eq!(
            parse_args(["region", "/tmp/b.png", "--live"]).unwrap(),
            Command::Run {
                mode: Mode::Region,
                live: true,
                file: PathBuf::from("/tmp/b.png"),
            }
        );
        assert_eq!(
            parse_args(["window", "/tmp/c.png"]).unwrap(),
            Command::Run {
                mode: Mode::Window,
                live: false,
                file: PathBuf::from("/tmp/c.png"),
            }
        );
    }

    #[test]
    fn rejects_unknown_mode_and_bad_args_with_one_line_error() {
        for bad in [
            vec![],
            vec!["bogus", "/tmp/a.png"],
            vec!["screen"],
            vec!["screen", "--live", "/tmp/a.png"],
            vec!["region", "--bogus", "/tmp/a.png"],
            vec!["region", "/tmp/a.png", "/tmp/b.png"],
        ] {
            let err = parse_args(bad).unwrap_err();
            assert!(!err.contains('\n'), "error must be single line: {err}");
        }
    }
}
