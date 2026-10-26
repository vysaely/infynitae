//! Input methods (fcitx5, ibus): `zwp_input_method_v2` for the input method, `zwp_text_input_v3`
//! for the apps it types into, `zwp_virtual_keyboard_v1` for the keys it passes on. Smithay
//! serves the three protocols; the requests that can type or read text pass through the gates
//! here first.
//!
//! A virtual keyboard that uploads the seat's own keymap (the remote desktop portal) is a real
//! keyboard: its keys go to Core, so the shortcuts work (`virtual_input`). Others type through
//! smithay into the focused window.
//!
//! While the session is locked nothing reaches the lock screen except your own keys (owner,
//! 2026-10-25): an input method can neither commit, preedit nor delete text, a virtual keyboard
//! types nothing, and no text input is enabled, so no app's text, the password field included,
//! goes to the input method. The keys themselves go to the lock surface past any keyboard grab
//! (`lock.rs`), so an input method that grabbed the keyboard does not see the password.
//!
//! The input method's popup (the candidate list) is drawn under the text cursor of the window it
//! types into, at that window's zoom.

use smithay::reexports::wayland_protocols::wp::text_input::zv3::server::zwp_text_input_v3::{
    self, ZwpTextInputV3,
};
use smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_v2::{
    self, ZwpInputMethodV2,
};
use smithay::reexports::wayland_protocols_misc::zwp_virtual_keyboard_v1::server::zwp_virtual_keyboard_v1::{
    self, ZwpVirtualKeyboardV1,
};
use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, DataInit, Dispatch, DisplayHandle, Resource};
use smithay::wayland::input_method::{
    InputMethodHandler, InputMethodManagerState, InputMethodUserData, PopupSurface,
};
use smithay::wayland::text_input::{TextInputManagerState, TextInputUserData};
use smithay::wayland::virtual_keyboard::{VirtualKeyboardManagerState, VirtualKeyboardUserData};

use crate::WaylandState;

/// The three globals and the input method's popups.
pub struct InputMethodState {
    #[expect(
        dead_code,
        reason = "owns the zwp_input_method_manager_v2 global for the state's lifetime"
    )]
    input_method: InputMethodManagerState,
    #[expect(
        dead_code,
        reason = "owns the zwp_text_input_manager_v3 global for the state's lifetime"
    )]
    text_input: TextInputManagerState,
    #[expect(
        dead_code,
        reason = "owns the zwp_virtual_keyboard_manager_v1 global for the state's lifetime"
    )]
    virtual_keyboard: VirtualKeyboardManagerState,
    pub(crate) popups: Vec<PopupSurface>,
}

impl InputMethodState {
    pub fn new(display: &DisplayHandle) -> Self {
        Self {
            input_method: InputMethodManagerState::new::<WaylandState, _>(display, |_| true),
            text_input: TextInputManagerState::new::<WaylandState>(display),
            virtual_keyboard: VirtualKeyboardManagerState::new::<WaylandState, _>(display, |_| {
                true
            }),
            popups: Vec::new(),
        }
    }
}

impl InputMethodHandler for WaylandState {
    fn new_popup(&mut self, surface: PopupSurface) {
        self.input_method.popups.push(surface);
    }

    fn dismiss_popup(&mut self, surface: PopupSurface) {
        self.input_method
            .popups
            .retain(|p| p.wl_surface() != surface.wl_surface());
    }

    fn popup_repositioned(&mut self, _surface: PopupSurface) {}

    /// Where the popup goes is decided when it is drawn (`convert::input_method_popups`), from
    /// the text cursor and the window's place on the canvas: nothing to give here.
    fn parent_geometry(&self, _parent: &WlSurface) -> crate::convert::InputPopupParentGeometry {
        Default::default()
    }
}

impl WaylandState {
    /// The input method's popups get their frame callbacks with the output's frame.
    pub(crate) fn complete_input_method_frames(
        &mut self,
        output: &smithay::output::Output,
        now: std::time::Duration,
    ) {
        self.input_method.popups.retain(|p| p.alive());
        for p in &self.input_method.popups {
            smithay::desktop::utils::send_frames_surface_tree(
                p.wl_surface(),
                output,
                now,
                None,
                |_, _| Some(output.clone()),
            );
        }
    }
}

smithay::reexports::wayland_server::delegate_global_dispatch!(WaylandState: [
    smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_manager_v2::ZwpInputMethodManagerV2:
    smithay::wayland::input_method::InputMethodManagerGlobalData
] => InputMethodManagerState);
smithay::reexports::wayland_server::delegate_dispatch!(WaylandState: [
    smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_manager_v2::ZwpInputMethodManagerV2: ()
] => InputMethodManagerState);
smithay::reexports::wayland_server::delegate_dispatch!(WaylandState: [
    smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2:
    smithay::wayland::input_method::InputMethodKeyboardUserData<WaylandState>
] => InputMethodManagerState);
smithay::reexports::wayland_server::delegate_dispatch!(WaylandState: [
    smithay::reexports::wayland_protocols_misc::zwp_input_method_v2::server::zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2:
    smithay::wayland::input_method::InputMethodPopupSurfaceUserData
] => InputMethodManagerState);
smithay::reexports::wayland_server::delegate_global_dispatch!(WaylandState: [
    smithay::reexports::wayland_protocols::wp::text_input::zv3::server::zwp_text_input_manager_v3::ZwpTextInputManagerV3: ()
] => TextInputManagerState);
smithay::reexports::wayland_server::delegate_dispatch!(WaylandState: [
    smithay::reexports::wayland_protocols::wp::text_input::zv3::server::zwp_text_input_manager_v3::ZwpTextInputManagerV3: ()
] => TextInputManagerState);
smithay::reexports::wayland_server::delegate_global_dispatch!(WaylandState: [
    smithay::reexports::wayland_protocols_misc::zwp_virtual_keyboard_v1::server::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1:
    smithay::wayland::virtual_keyboard::VirtualKeyboardManagerGlobalData
] => VirtualKeyboardManagerState);
smithay::reexports::wayland_server::delegate_dispatch!(WaylandState: [
    smithay::reexports::wayland_protocols_misc::zwp_virtual_keyboard_v1::server::zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1: ()
] => VirtualKeyboardManagerState);

/// The input method: while locked, nothing that changes text gets through. Requests that create
/// an object (the keyboard grab, a popup) always do, the protocol needs them answered; the grab
/// then sees no key while locked (`lock.rs`).
impl Dispatch<ZwpInputMethodV2, InputMethodUserData<WaylandState>> for WaylandState {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &ZwpInputMethodV2,
        request: zwp_input_method_v2::Request,
        data: &InputMethodUserData<WaylandState>,
        dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use zwp_input_method_v2::Request;
        if state.is_locked()
            && matches!(
                request,
                Request::CommitString { .. }
                    | Request::SetPreeditString { .. }
                    | Request::DeleteSurroundingText { .. }
                    | Request::Commit { .. }
            )
        {
            return;
        }
        <InputMethodManagerState as Dispatch<
            ZwpInputMethodV2,
            InputMethodUserData<WaylandState>,
            WaylandState,
        >>::request(state, client, resource, request, data, dh, data_init);
    }

    fn destroyed(
        state: &mut Self,
        client: ClientId,
        resource: &ZwpInputMethodV2,
        data: &InputMethodUserData<WaylandState>,
    ) {
        <InputMethodManagerState as Dispatch<
            ZwpInputMethodV2,
            InputMethodUserData<WaylandState>,
            WaylandState,
        >>::destroyed(state, client, resource, data);
    }
}

/// An app's text input: while locked, nothing is enabled or sent to the input method.
impl Dispatch<ZwpTextInputV3, TextInputUserData> for WaylandState {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &ZwpTextInputV3,
        request: zwp_text_input_v3::Request,
        data: &TextInputUserData,
        dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if state.is_locked() && !matches!(request, zwp_text_input_v3::Request::Destroy) {
            return;
        }
        <TextInputManagerState as Dispatch<ZwpTextInputV3, TextInputUserData, WaylandState>>::request(
            state, client, resource, request, data, dh, data_init,
        );
    }

    fn destroyed(
        state: &mut Self,
        client: ClientId,
        resource: &ZwpTextInputV3,
        data: &TextInputUserData,
    ) {
        <TextInputManagerState as Dispatch<ZwpTextInputV3, TextInputUserData, WaylandState>>::destroyed(
            state, client, resource, data,
        );
    }
}

/// A virtual keyboard: while locked it types nothing (it would type into the lock screen). One
/// with the seat's keymap types through Core.
impl Dispatch<ZwpVirtualKeyboardV1, VirtualKeyboardUserData<WaylandState>> for WaylandState {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &ZwpVirtualKeyboardV1,
        request: zwp_virtual_keyboard_v1::Request,
        data: &VirtualKeyboardUserData<WaylandState>,
        dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        use zwp_virtual_keyboard_v1::Request;
        let locked = state.is_locked();
        match &request {
            Request::Keymap { fd, size, .. } => {
                // read from a copy: smithay maps the keymap from the fd itself
                let text = fd.try_clone().ok().and_then(|fd| {
                    use std::os::unix::fs::FileExt;
                    let mut buf = vec![0; (*size as usize).min(4 << 20)];
                    std::fs::File::from(fd).read_exact_at(&mut buf, 0).ok()?;
                    Some(buf)
                });
                state
                    .virtual_input
                    .keyboard_keymap(resource.id(), text.as_deref().unwrap_or_default());
            }
            Request::Key {
                key,
                state: key_state,
                ..
            } => {
                // wl_keyboard.key_state: 1 pressed, 0 released
                let pressed = *key_state == 1;
                if state
                    .virtual_input
                    .keyboard_key(&resource.id(), *key, pressed, locked)
                {
                    return;
                }
            }
            Request::Modifiers { .. } if state.virtual_input.is_seat_keyboard(&resource.id()) => {
                return;
            }
            _ => {}
        }
        if locked && matches!(request, Request::Key { .. } | Request::Modifiers { .. }) {
            return;
        }
        <VirtualKeyboardManagerState as Dispatch<
            ZwpVirtualKeyboardV1,
            VirtualKeyboardUserData<WaylandState>,
            WaylandState,
        >>::request(state, client, resource, request, data, dh, data_init);
    }

    fn destroyed(
        state: &mut Self,
        client: ClientId,
        resource: &ZwpVirtualKeyboardV1,
        data: &VirtualKeyboardUserData<WaylandState>,
    ) {
        state.virtual_input.release_keyboard(&resource.id());
        <VirtualKeyboardManagerState as Dispatch<
            ZwpVirtualKeyboardV1,
            VirtualKeyboardUserData<WaylandState>,
            WaylandState,
        >>::destroyed(state, client, resource, data);
    }
}
