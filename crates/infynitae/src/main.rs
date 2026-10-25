//! Infynitae Native composition root: creates Core and its authority ports, the adapter, the
//! backend and the event loop, and hands each port to its only consumer (FOUNDATION_ARCHITECTURE
//! C1.2, C1.5).

#![deny(unsafe_code)]

use std::process::ExitCode;
use std::time::Duration;

use infynitae_core::Core;
use infynitae_core::navigation::PersistedCameras;

mod shell;

fn find_xwayland_satellite() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("INFYNITAE_XWAYLAND_SATELLITE") {
        let p = std::path::PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("xwayland-satellite");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for standard in [
        "/usr/local/bin/xwayland-satellite",
        "/usr/bin/xwayland-satellite",
    ] {
        let p = std::path::PathBuf::from(standard);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

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
    --filter <nearest|linear> Texture scaling filter (default nearest for 0% blur)
    --duration <SECONDS>     Exit after N seconds (useful for tests and automation)
    --max-frames <COUNT>     Exit after N frames (useful for benchmarks and tests)
    --activation-policy <strict|attention-only|permissive>
                             What xdg-activation requests may do (default strict: focus only
                             with a token from the focused client; otherwise attention)
    --shell-socket <PATH>    The trusted shell's socket (default $XDG_RUNTIME_DIR/infynitae/shell.sock;
                             JSON lines, FOUNDATION_ARCHITECTURE C7)
    --no-shell               No shell socket
    --no-xwayland            Disable X11 / Xwayland support even if xwayland-satellite is found
    --xwayland-satellite <PATH>
                             Supervise this xwayland-satellite binary on an X display.
                             Auto-detected from PATH or system locations by default if installed
"
    );
}

fn main() -> ExitCode {
    // Install panic hook to capture crash backtrace into ~/.local/state/infynitae/crash.log
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let home = std::env::var("HOME").unwrap_or_default();
        let crash_dir = format!("{home}/.local/state/infynitae");
        let _ = std::fs::create_dir_all(&crash_dir);
        let crash_file = format!("{crash_dir}/crash.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&crash_file)
        {
            use std::io::Write;
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "=== Infynitae Crash at Unix Timestamp {timestamp} ===");
            let _ = writeln!(f, "{info}");
            let _ = writeln!(f, "Backtrace:\n{:?}", std::backtrace::Backtrace::capture());
            let _ = writeln!(f, "====================================================\n");
        }
        default_hook(info);
    }));

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
    let mut scaling_filter: Option<String> = None;
    let mut duration: Option<Duration> = None;
    let mut max_frames: Option<u64> = None;
    let mut xwayland_satellite: Option<std::path::PathBuf> = None;
    let mut no_xwayland = false;
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
            "--filter" | "--scaling-filter" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --filter requires an argument (nearest or linear)");
                    return ExitCode::FAILURE;
                }
                scaling_filter = Some(args[i].to_lowercase());
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
                match parse_duration(&args[i]) {
                    Ok(d) => duration = Some(d),
                    Err(_) => {
                        eprintln!("Error: invalid duration '{}'", args[i]);
                        return ExitCode::FAILURE;
                    }
                }
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
            "--no-xwayland" => {
                no_xwayland = true;
                xwayland_satellite = None;
            }
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

    if let Ok(xw) = std::env::var("XWAYLAND")
        && matches!(xw.to_lowercase().as_str(), "0" | "false" | "off" | "no")
    {
        no_xwayland = true;
        xwayland_satellite = None;
    }

    if !no_xwayland && xwayland_satellite.is_none() {
        xwayland_satellite = find_xwayland_satellite();
    }

    // Composition root: creates Core and authority ports (FOUNDATION_ARCHITECTURE C1.2, C1.5)
    let (mut core, input_port, shell_port, output_token) = Core::new(PersistedCameras::default());
    core.set_activation_policy(activation_policy);
    if let Ok(mode) = std::env::var("PAN_MODE") {
        match mode.to_lowercase().as_str() {
            "rts" => core.set_pan_mode(infynitae_core::navigation::PanMode::Rts),
            "free" => core.set_pan_mode(infynitae_core::navigation::PanMode::Free),
            _ => core.set_pan_mode(infynitae_core::navigation::PanMode::Grab),
        }
    }
    core.set_focus_follows_mouse(true);
    core.set_focus_follows_mouse_strict(false);
    core.set_focus_cycle_navigate(true);
    core.set_placement_policy(infynitae_core::PlacementPolicy::ClusterTiling);
    core.set_enter_mode(infynitae_core::EnterMode::Maximize);
    core.set_fullscreen_pan(true);
    core.set_focus_on_open(true);
    core.set_teleport_on_open(true);

    let mut direct_scanout: Option<bool> = None;
    let mut vrr: Option<bool> = None;

    if let Ok(fp) = std::env::var("FULLSCREEN_PAN") {
        core.set_fullscreen_pan(!matches!(
            fp.to_lowercase().as_str(),
            "0" | "false" | "off" | "no" | "exclusive"
        ));
    }

    if let Ok(sea) = std::env::var("SUPER_ENTER_ACTION") {
        if sea.eq_ignore_ascii_case("fit") {
            core.set_enter_mode(infynitae_core::EnterMode::Fit);
        } else {
            core.set_enter_mode(infynitae_core::EnterMode::Maximize);
        }
    }

    if let Ok(ffm) = std::env::var("FOCUS_FOLLOWS_MOUSE") {
        match ffm.to_lowercase().as_str() {
            "strict" => {
                core.set_focus_follows_mouse(true);
                core.set_focus_follows_mouse_strict(true);
            }
            "0" | "false" | "off" | "no" | "click" => {
                core.set_focus_follows_mouse(false);
                core.set_focus_follows_mouse_strict(false);
            }
            _ => {
                core.set_focus_follows_mouse(true);
                core.set_focus_follows_mouse_strict(false);
            }
        }
    }
    if let Ok(fcn) = std::env::var("FOCUS_CYCLE_NAVIGATE") {
        core.set_focus_cycle_navigate(!matches!(
            fcn.to_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ));
    }
    if let Ok(val) = std::env::var("FOCUS_ON_OPEN") {
        core.set_focus_on_open(!matches!(
            val.to_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ));
    }
    if let Ok(too) = std::env::var("TELEPORT_ON_OPEN") {
        core.set_teleport_on_open(!matches!(
            too.to_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ));
    }
    if let Ok(home) = std::env::var("HOME") {
        let config_path = format!("{home}/.config/infynitae/config");
        if let Ok(content) = std::fs::read_to_string(config_path) {
            for line in content.lines() {
                let line_clean = line.split('#').next().unwrap_or("").trim();
                if let Some((k, v)) = line_clean.split_once('=') {
                    let key = k.trim();
                    let val = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
                    match key {
                        "PAN_MODE" => match val.to_lowercase().as_str() {
                            "rts" => core.set_pan_mode(infynitae_core::navigation::PanMode::Rts),
                            "free" => core.set_pan_mode(infynitae_core::navigation::PanMode::Free),
                            _ => core.set_pan_mode(infynitae_core::navigation::PanMode::Grab),
                        },
                        "FOCUS_FOLLOWS_MOUSE" => match val.to_lowercase().as_str() {
                            "strict" => {
                                core.set_focus_follows_mouse(true);
                                core.set_focus_follows_mouse_strict(true);
                            }
                            "0" | "false" | "off" | "no" | "click" => {
                                core.set_focus_follows_mouse(false);
                                core.set_focus_follows_mouse_strict(false);
                            }
                            _ => {
                                core.set_focus_follows_mouse(true);
                                core.set_focus_follows_mouse_strict(false);
                            }
                        },
                        "FOCUS_CYCLE_NAVIGATE" => {
                            let enabled = !matches!(
                                val.to_lowercase().as_str(),
                                "0" | "false" | "off" | "no"
                            );
                            core.set_focus_cycle_navigate(enabled);
                        }
                        "FOCUS_ON_OPEN" => {
                            let enabled = !matches!(
                                val.to_lowercase().as_str(),
                                "0" | "false" | "off" | "no"
                            );
                            core.set_focus_on_open(enabled);
                        }
                        "TELEPORT_ON_OPEN" => {
                            let enabled = !matches!(
                                val.to_lowercase().as_str(),
                                "0" | "false" | "off" | "no"
                            );
                            core.set_teleport_on_open(enabled);
                        }
                        "PLACEMENT_POLICY" => match val.to_lowercase().as_str() {
                            "center" | "centred" | "centered" => core.set_placement_policy(
                                infynitae_core::PlacementPolicy::CentredInView,
                            ),
                            "origin" => core.set_placement_policy(
                                infynitae_core::PlacementPolicy::OriginAtCameraCentre,
                            ),
                            _ => core.set_placement_policy(
                                infynitae_core::PlacementPolicy::ClusterTiling,
                            ),
                        },
                        "SUPER_ENTER_ACTION" => {
                            if val.eq_ignore_ascii_case("fit") {
                                core.set_enter_mode(infynitae_core::EnterMode::Fit);
                            } else {
                                core.set_enter_mode(infynitae_core::EnterMode::Maximize);
                            }
                        }
                        "FULLSCREEN_PAN" => {
                            let enabled = !matches!(
                                val.to_lowercase().as_str(),
                                "0" | "false" | "off" | "no" | "exclusive"
                            );
                            core.set_fullscreen_pan(enabled);
                        }
                        "OUTPUT_SCALE" => {
                            if scale_120.is_none()
                                && let Ok(f) = val.parse::<f64>()
                                && (0.25..=8.0).contains(&f)
                            {
                                scale_120 = Some((f * 120.0).round().max(1.0) as u32);
                            }
                        }
                        "SCALING_FILTER" => {
                            if scaling_filter.is_none() {
                                scaling_filter = Some(val.to_lowercase());
                            }
                        }
                        "DIRECT_SCANOUT" => {
                            if direct_scanout.is_none() {
                                direct_scanout = Some(!matches!(
                                    val.to_lowercase().as_str(),
                                    "0" | "false" | "off" | "no"
                                ));
                            }
                        }
                        "VRR_ENABLED" | "VRR" | "ADAPTIVE_SYNC" if vrr.is_none() => {
                            match val.to_lowercase().as_str() {
                                // auto (None): only while a window is fullscreen
                                "on" | "true" | "1" | "yes" => vrr = Some(true),
                                "off" | "false" | "0" | "no" => vrr = Some(false),
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    if direct_scanout.is_none()
        && let Ok(val) = std::env::var("DIRECT_SCANOUT")
    {
        direct_scanout = Some(!matches!(
            val.to_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ));
    }
    if vrr.is_none()
        && let Ok(val) = std::env::var("VRR_ENABLED")
            .or_else(|_| std::env::var("VRR"))
            .or_else(|_| std::env::var("ADAPTIVE_SYNC"))
    {
        match val.to_lowercase().as_str() {
            "on" | "true" | "1" | "yes" => vrr = Some(true),
            "off" | "false" | "0" | "no" => vrr = Some(false),
            _ => {}
        }
    }
    if scale_120.is_none()
        && let Ok(val) = std::env::var("OUTPUT_SCALE")
        && let Ok(f) = val.parse::<f64>()
        && (0.25..=8.0).contains(&f)
    {
        scale_120 = Some((f * 120.0).round().max(1.0) as u32);
    }
    if scaling_filter.is_none()
        && let Ok(val) = std::env::var("SCALING_FILTER")
    {
        scaling_filter = Some(val.to_lowercase());
    }
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
        let exec = exec_cmd.or_else(|| {
            let home = std::env::var("HOME").ok()?;
            let session_script = format!("{home}/infynitae/live/session.sh");
            if std::path::Path::new(&session_script).exists() {
                Some(session_script)
            } else {
                None
            }
        });
        let options = infynitae_backend_drm::ConsoleOptions {
            socket_name,
            exec,
            duration,
            max_frames,
            xwayland_satellite,
            scale_120,
            scaling_filter: scaling_filter.clone(),
            direct_scanout,
            vrr,
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
            scale_120,
            scaling_filter,
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

/// Maximum accepted run duration (365 days in seconds).
const MAX_DURATION_SECS: f64 = 365.0 * 24.0 * 3600.0;

/// Parses a duration argument in seconds.
///
/// Accepts non-negative finite floating-point values up to [`MAX_DURATION_SECS`].
fn parse_duration(s: &str) -> Result<Duration, &'static str> {
    let secs: f64 = s.parse().map_err(|_| "not a number")?;
    if !secs.is_finite() || secs < 0.0 {
        return Err("duration must be finite and non-negative");
    }
    if secs > MAX_DURATION_SECS {
        return Err("duration exceeds maximum allowed");
    }
    Duration::try_from_secs_f64(secs).map_err(|_| "duration overflow")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_valid() {
        assert_eq!(parse_duration("0").unwrap(), Duration::from_secs(0));
        assert_eq!(parse_duration("0.0").unwrap(), Duration::from_secs(0));
        assert_eq!(parse_duration("1.5").unwrap(), Duration::from_millis(1500));
        assert_eq!(parse_duration("60").unwrap(), Duration::from_secs(60));
        assert_eq!(
            parse_duration("31536000").unwrap(),
            Duration::from_secs(31536000)
        );
    }

    #[test]
    fn test_parse_duration_rejects_invalid() {
        // Negative
        assert!(parse_duration("-1").is_err());
        assert!(parse_duration("-0.001").is_err());

        // Non-finite
        assert!(parse_duration("nan").is_err());
        assert!(parse_duration("NaN").is_err());
        assert!(parse_duration("inf").is_err());
        assert!(parse_duration("+inf").is_err());
        assert!(parse_duration("-inf").is_err());

        // Above upper bound
        assert!(parse_duration("31536001").is_err());
        assert!(parse_duration("1e30").is_err());

        // Not a number
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("").is_err());
    }
}
