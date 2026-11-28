//! `infynitae-health`: system health snapshot, 60 s JSONL sampler, and condensed event log
//! (documented in `docs/HEALTH.md`).

mod collect;
mod history;
mod log;
mod model;
mod sys;

use model::parse_since_arg;
use std::process::ExitCode;
use sys::SysEnv;

fn print_usage() {
    eprintln!(
        "\
Usage:
  infynitae-health now [--json]
  infynitae-health log [--since DURATION|TIMESTAMP] [--json]
  infynitae-health sample [--no-notify] [--json]

Durations: 30m, 1h (default), 24h, 7d, or an ISO timestamp (2026-11-28T07:00:00Z)."
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Command {
    Now { json: bool },
    Log { since: String, json: bool },
    Sample { notify: bool, json: bool },
    Help,
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    let Some(first) = args.first().map(String::as_str) else {
        return Err("missing subcommand (expected now, log, or sample)".into());
    };
    match first {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "now" => {
            let mut json = false;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--json" => json = true,
                    "-h" | "--help" => return Ok(Command::Help),
                    other => return Err(format!("unknown option for now: {other}")),
                }
            }
            Ok(Command::Now { json })
        }
        "log" => {
            let mut since = "1h".to_string();
            let mut json = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--json" => json = true,
                    "--since" => {
                        i += 1;
                        let Some(v) = args.get(i) else {
                            return Err("--since requires a value".into());
                        };
                        since = v.clone();
                    }
                    s if s.starts_with("--since=") => {
                        since = s["--since=".len()..].to_string();
                    }
                    "-h" | "--help" => return Ok(Command::Help),
                    other => return Err(format!("unknown option for log: {other}")),
                }
                i += 1;
            }
            Ok(Command::Log { since, json })
        }
        "sample" => {
            let mut notify = true;
            let mut json = false;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--no-notify" => notify = false,
                    "--json" => json = true,
                    "-h" | "--help" => return Ok(Command::Help),
                    other => return Err(format!("unknown option for sample: {other}")),
                }
            }
            Ok(Command::Sample { notify, json })
        }
        other => Err(format!("unknown subcommand: {other}")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("infynitae-health: {e}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    match cmd {
        Command::Help => {
            print_usage();
            ExitCode::SUCCESS
        }
        Command::Now { json } => {
            let env = SysEnv::system(false);
            let snap = collect::collect_snapshot(&env);
            if json {
                match serde_json::to_string_pretty(&snap) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("infynitae-health: json error: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                print!("{}", collect::render_now_text(&snap));
            }
            ExitCode::SUCCESS
        }
        Command::Log { since, json } => {
            let env = SysEnv::system(false);
            let Some(since_unix) = parse_since_arg(&since, env.now_unix) else {
                eprintln!("infynitae-health: invalid --since value: {since}");
                return ExitCode::from(2);
            };
            let report = log::collect_log(&env, since_unix);
            if json {
                match serde_json::to_string_pretty(&report) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("infynitae-health: json error: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                print!("{}", log::render_log_text(&report));
            }
            ExitCode::SUCCESS
        }
        Command::Sample { notify, json } => {
            let env = SysEnv::system(true);
            let snap = collect::collect_snapshot(&env);
            if let Err(e) = history::append_and_rotate(&env.health_dir(), &snap, history::KEEP_DAYS)
            {
                eprintln!("infynitae-health: failed to write sample: {e}");
                return ExitCode::FAILURE;
            }
            if notify {
                let _ = history::notify_new_problems(&env, &snap.items);
            }
            if json {
                match serde_json::to_string(&snap) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("infynitae-health: json error: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            ExitCode::SUCCESS
        }
    }
}

#[cfg(test)]
mod tests;
