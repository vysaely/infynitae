#![no_main]

use infynitae_edge_apps::clipboard_entry::{
    copy_command, delete_command, kind_of, parse_cliphist_with_home,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    for item in parse_cliphist_with_home(s, Some("/home/alice")) {
        let _ = kind_of(&item);
        let _ = copy_command(&item.raw_line);
        let _ = delete_command(&item.raw_line);
    }
});
