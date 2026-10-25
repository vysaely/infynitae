//! `ext-idle-notify-v1` protocol implementation.
//! Allows external idle tools (swayidle, hypridle) to monitor user idle status.

use std::time::Duration;

use smithay::reexports::wayland_protocols::ext::idle_notify::v1::server::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::{self, ExtIdleNotifierV1},
};
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
    backend::{GlobalId, ObjectId},
};

use crate::WaylandState;

/// An active idle notification registration.
#[derive(Debug)]
pub struct IdleNotification {
    pub resource: ExtIdleNotificationV1,
    pub timeout: Duration,
    pub ignore_inhibitor: bool,
    pub is_idled: bool,
}

/// Global idle-notifier state held by `WaylandState`.
pub struct IdleNotifyState {
    pub global: GlobalId,
    pub notifications: Vec<IdleNotification>,
    pub last_activity: Duration,
}

impl IdleNotifyState {
    /// Advertises `ext_idle_notifier_v1` version 2 (`get_input_idle_notification`).
    pub fn new(dh: &DisplayHandle) -> Self {
        let global = dh.create_global::<WaylandState, ExtIdleNotifierV1, ()>(2, ());
        Self {
            global,
            notifications: Vec::new(),
            last_activity: crate::presentation_clock_now(),
        }
    }

    /// Drops a notification destroyed by the client.
    pub fn remove_notification(&mut self, id: ObjectId) {
        self.notifications.retain(|n| n.resource.id() != id);
    }
}

impl GlobalDispatch<ExtIdleNotifierV1, ()> for WaylandState {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ExtIdleNotifierV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ExtIdleNotifierV1,
        request: ext_idle_notifier_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            ext_idle_notifier_v1::Request::GetIdleNotification {
                id,
                timeout,
                seat: _,
            } => {
                let timeout = Duration::from_millis(timeout as u64);
                let notification = data_init.init(id, ());
                state.idle_notify.notifications.push(IdleNotification {
                    resource: notification,
                    timeout,
                    ignore_inhibitor: false,
                    is_idled: false,
                });
            }
            ext_idle_notifier_v1::Request::GetInputIdleNotification {
                id,
                timeout,
                seat: _,
            } => {
                let timeout = Duration::from_millis(timeout as u64);
                let notification = data_init.init(id, ());
                state.idle_notify.notifications.push(IdleNotification {
                    resource: notification,
                    timeout,
                    ignore_inhibitor: true,
                    is_idled: false,
                });
            }
            ext_idle_notifier_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for WaylandState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ExtIdleNotificationV1,
        request: ext_idle_notification_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let ext_idle_notification_v1::Request::Destroy = request {
            state.idle_notify.remove_notification(resource.id());
        }
    }
}
