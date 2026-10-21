//! Infynitae Native composition root: creates Core and its authority ports, the adapter, the
//! backend and the event loop, and hands each port to its only consumer (FOUNDATION_ARCHITECTURE
//! C1.2, C1.5).

#![deny(unsafe_code)]

use std::process::ExitCode;
use std::time::Duration;

use infynitae_core::Core;
use infynitae_core::navigation::PersistedCameras;

mod shell;

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
    --tty                    Run on the console: own the screens and input devices of the seat
                             (EXPERIMENTAL). The default when started with neither WAYLAND_DISPLAY
                             nor DISPLAY set, i.e. from a text console. Ctrl+Alt+Backspace quits;
                             Ctrl+Alt+F1..F12 switch VT
    --nested                 Run as a window of the running desktop even without WAYLAND_DISPLAY
    --scale <FACTOR>         Console screens' scale (e.g. 1.5; default 1)
    --duration <SECONDS>     Exit after N seconds (useful for tests and automation)
    --max-frames <COUNT>     Exit after N frames (useful for benchmarks and tests)
    --activation-policy <strict|attention-only|permissive>
                             What xdg-activation requests may do (default strict: focus only
                             with a token from the focused client; otherwise attention)
    --shell-socket <PATH>    The trusted shell's socket (default $XDG_RUNTIME_DIR/infynitae/shell.sock;
                             JSON lines, FOUNDATION_ARCHITECTURE C7)
    --no-shell               No shell socket
    --xwayland-satellite <PATH>
                             EXPERIMENTAL (INC-12): supervise this xwayland-satellite on an X
                             display the compositor owns; clients get it as DISPLAY. Off by
                             default; without it clients get no DISPLAY
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
    let mut tty: Option<bool> = None;
    let mut scale_120: Option<u32> = None;
    let mut duration: Option<Duration> = None;
    let mut max_frames: Option<u64> = None;
    let mut xwayland_satellite: Option<std::path::PathBuf> = None;
    let mut activation_policy = infynitae_core::input::focus::ActivationPolicy::Strict;
    let mut shell_socket = shell::default_path();

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
            "--tty" => tty = Some(true),
            "--nested" => tty = Some(false),
            "--scale" => {
                i += 1;
                let factor: Option<f64> = args.get(i).and_then(|a| a.parse().ok());
                match factor.filter(|f| (0.25..=8.0).contains(f)) {
                    Some(f) => scale_120 = Some((f * 120.0).round() as u32),
                    None => {
                        eprintln!("Error: --scale needs a factor from 0.25 to 8 (e.g. 1.5)");
                        return ExitCode::FAILURE;
                    }
                }
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
            "--activation-policy" => {
                use infynitae_core::input::focus::ActivationPolicy;
                i += 1;
                activation_policy = match args.get(i).map(String::as_str) {
                    Some("strict") => ActivationPolicy::Strict,
                    Some("attention-only") => ActivationPolicy::AttentionOnly,
                    Some("permissive") => ActivationPolicy::Permissive,
                    _ => {
                        eprintln!(
                            "Error: --activation-policy takes strict, attention-only or permissive"
                        );
                        return ExitCode::FAILURE;
                    }
                };
            }
            "--shell-socket" => {
                i += 1;
                let Some(p) = args.get(i) else {
                    eprintln!("Error: --shell-socket requires a path");
                    return ExitCode::FAILURE;
                };
                shell_socket = Some(p.into());
            }
            "--no-shell" => shell_socket = None,
            "--xwayland-satellite" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --xwayland-satellite requires a path");
                    return ExitCode::FAILURE;
                }
                xwayland_satellite = Some(args[i].clone().into());
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
    let (mut core, input_port, shell_port, output_token) = Core::new(PersistedCameras::default());
    core.set_activation_policy(activation_policy);
    // the ShellPort's only consumer is the shell server (C7.4); without a socket it is dropped
    let extension = shell_socket.map(|path| shell::factory(shell_port, path));

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
            xwayland_satellite: xwayland_satellite.clone(),
        };
        match infynitae_backend_headless::run_headless_with(
            core,
            input_port,
            output_token,
            options,
            extension,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[infynitae] Headless compositor error: {e}");
                ExitCode::FAILURE
            }
        }
    } else if tty.unwrap_or_else(|| {
        std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none()
    }) {
        println!("[infynitae] Starting console compositor (DRM backend, EXPERIMENTAL)...");
        let options = infynitae_backend_drm::ConsoleOptions {
            socket_name,
            exec: exec_cmd,
            duration,
            max_frames,
            xwayland_satellite,
            scale_120,
        };
        match infynitae_backend_drm::run_console_with(
            core,
            input_port,
            output_token,
            options,
            extension,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[infynitae] Console compositor error: {e}");
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
            xwayland_satellite,
        };
        match infynitae_backend_winit::run_nested_with(
            core,
            input_port,
            output_token,
            options,
            extension,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("[infynitae] Nested compositor error: {e}");
                ExitCode::FAILURE
            }
        }
    }
}
