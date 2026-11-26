#![no_main]

use infynitae_core::navigation::PersistedCameras;
use infynitae_core::Core;
use infynitae_ipc::json;
use infynitae_ipc::protocol::{error_reply, ok_reply, parse_request};
use libfuzzer_sys::fuzz_target;

thread_local! {
    static CORE: Core = Core::new(PersistedCameras::default()).0;
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let _ = json::parse(s);
    CORE.with(|core| {
        let (id, res) = parse_request(s, core);
        let _ = match res {
            Ok(_) => ok_reply(id, 0, None),
            Err(e) => error_reply(id, e),
        };
    });
});
