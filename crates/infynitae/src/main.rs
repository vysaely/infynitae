//! Infynitae Native composition root: creates Core and its authority ports, the adapter, the
//! backend and the event loop, and hands each port to its only consumer (FOUNDATION_ARCHITECTURE
//! C1.2, C1.5).

#![deny(unsafe_code)]

use std::process::ExitCode;
use std::time::Duration;

use infynitae_core::Core;
use infynitae_core::navigation::PersistedCameras;

fn print_help() {
    println!(
        "\
Infynitae Native Compositor

USAGE:
    infynitae [OPTIONS]

OPTIONS:
    -h, --help               Print help information
    --socket <NAME>          Set Wayland socket name (e.g. wayland-1, or auto by default)
    --exec <CMD>             Command to execute once compositor is ready (e.g. weston-terminal)
    --title <TITLE>          Title of the nested window (default: \"Infynitae Native Compositor\")
    --width <PIXELS>         Window / output width (default: 1280)
    --height <PIXELS>        Window / output height (default: 800)
    --refresh <MHZ>          Output refresh rate in mHz (default: 60000 = 60 Hz)
    --headless               Run in headless mode with virtual output
    --duration <SECONDS>     Exit after N seconds (useful for tests and automation)
    --max-frames <COUNT>     Exit after N frames (useful for benchmarks and tests)
"
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut socket_name: Option<String> = None;
    let mut exec_cmd: Option<String> = None;
    let mut title = "Infynitae Native Compositor".to_string();
    let mut width = 1280u32;
    let mut height = 800u32;
    let mut refresh_mhz = 60_000u32;
    let mut headless = false;
    let mut duration: Option<Duration> = None;
    let mut max_frames: Option<u64> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            "--socket" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --socket requires an argument");
                    return ExitCode::FAILURE;
                }
                socket_name = Some(args[i].clone());
            }
            "--exec" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --exec requires an argument");
                    return ExitCode::FAILURE;
                }
                exec_cmd = Some(args[i].clone());
            }
            "--title" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --title requires an argument");
                    return ExitCode::FAILURE;
                }
                title = args[i].clone();
            }
            "--width" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --width requires an argument");
                    return ExitCode::FAILURE;
                }
                width = match args[i].parse() {
                    Ok(w) => w,
                    Err(_) => {
                        eprintln!("Error: invalid width '{}'", args[i]);
                        return ExitCode::FAILURE;
                    }
                };
            }
            "--height" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --height requires an argument");
                    return ExitCode::FAILURE;
                }
                height = match args[i].parse() {
                    Ok(h) => h,
                    Err(_) => {
                        eprintln!("Error: invalid height '{}'", args[i]);
                        return ExitCode::FAILURE;
                    }
                };
            }
            "--headless" => {
                headless = true;
            }
            "--refresh" => {
                i += 1;
                if i >= args.len() {
                    eprintln!(
                        "Error: --refresh requires an argument in mHz (e.g. 60000 or 144000)"
                    );
                    return ExitCode::FAILURE;
                }
                refresh_mhz = match args[i].parse() {
                    Ok(r) => r,
                    Err(_) => {
                        eprintln!("Error: invalid refresh '{}'", args[i]);
                        return ExitCode::FAILURE;
                    }
                };
            }
            "--duration" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --duration requires an argument in seconds");
                    return ExitCode::FAILURE;
                }
                let secs: f64 = match args[i].parse() {
                    Ok(s) => s,
                    Err(_) => {
                        eprintln!("Error: invalid duration '{}'", args[i]);
                        return ExitCode::FAILURE;
                    }
                };
                duration = Some(Duration::from_secs_f64(secs));
            }
            "--max-frames" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --max-frames requires an argument");
                    return ExitCode::FAILURE;
                }
                max_frames = match args[i].parse() {
                    Ok(n) => Some(n),
                    Err(_) => {
                        eprintln!("Error: invalid max-frames '{}'", args[i]);
                        return ExitCode::FAILURE;
                    }
                };
            }
            other => {
                eprintln!("Error: unknown argument '{other}'");
                print_help();
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }

    // Composition root: creates Core and authority ports (FOUNDATION_ARCHITECTURE C1.2, C1.5)
    let (core, input_port, _shell_port, output_token) = Core::new(PersistedCameras::default());

    if headless {
        println!("[infynitae] Starting headless compositor...");
        let options = infynitae_backend_headless::HeadlessCompositorOptions {
            socket_name,
            exec: exec_cmd,
            width,
            height,
            refresh_mhz,
            duration,
            max_frames,
            device_limit: infynitae_backend_headless::DeviceLimit::default(),
        };
        match infynitae_backend_headless::run_headless(core, input_port, output_token, options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[infynitae] Headless compositor error: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        println!("[infynitae] Starting nested compositor (Winit backend)...");
        let options = infynitae_backend_winit::WinitCompositorOptions {
            socket_name,
            exec: exec_cmd,
            title,
            width,
            height,
            refresh_mhz,
            duration,
            max_frames,
        };
        match infynitae_backend_winit::run_nested(core, input_port, output_token, options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[infynitae] Nested compositor error: {e}");
                ExitCode::FAILURE
            }
        }
    }
}
