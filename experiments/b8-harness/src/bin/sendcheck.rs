//! C-NB-2 feasibility: is a real Smithay 0.7.0 protocol-state struct `Send`? (compile-time check)
fn assert_send<T: Send>() {}
fn main() {
    assert_send::<b8_harness::server::Server>();
    println!("b8_harness::server::Server (Smithay 0.7.0 protocol state: compositor, xdg-shell, shm, outputs, seat, data-device, fractional-scale, viewporter, constraints, relative-pointer, presentation, dmabuf, PopupManager, Windows) is Send: compiles");
}
