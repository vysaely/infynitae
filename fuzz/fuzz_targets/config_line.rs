#![no_main]

use infynitae_wayland::{
    config_line, panel_key, parse_bool, parse_color_hex, parse_list, parse_titles, place_key,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    if let Some((k, v)) = config_line(s) {
        let _ = parse_bool(&v);
        let _ = parse_titles(&v);
        let _ = parse_list(&v);
        let _ = parse_color_hex(&v);
        let _ = panel_key(&k, &v);
        let _ = place_key(&k, &v);
    }
});
