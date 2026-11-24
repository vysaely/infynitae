//! Touchpad swipe, pinch and hold gestures.

use super::*;

impl Core {
    pub(super) fn gesture_swipe_begin(
        &mut self,
        fingers: u32,
        time_ms: u32,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        if self.input.gesture.is_some() || self.input.interaction.active() {
            return;
        }
        let PointerState::Located { output, .. } = self.pointer else {
            return;
        };
        let can_compositor = self.touchpad_gestures && !self.input.ownership.client_owns_any();
        if can_compositor && fingers == 3 {
            let presenting =
                self.presentation(output).is_some() && !self.is_presentation_suppressed(output);
            if !(presenting && !self.fullscreen_pan) && self.begin_camera_pan(output, 0, proof, out)
            {
                self.input.gesture = Some(TouchpadGesture::Pan);
            }
            return;
        }
        if can_compositor && fingers == 4 {
            self.input.gesture = Some(TouchpadGesture::FourFingerSwipe {
                dx: 0.0,
                dy: 0.0,
                triggered: false,
            });
            return;
        }
        self.input.gesture = Some(TouchpadGesture::Client(GestureKind::Swipe));
        out.push(Effect::GestureSwipeBegin { fingers, time_ms });
    }

    pub(super) fn gesture_swipe_update(
        &mut self,
        delta: (f64, f64),
        time_ms: u32,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        /// Logical swipe distance (upward) needed to trigger the overview on a 4-finger swipe.
        const SWIPE_OVERVIEW_THRESHOLD: f64 = 32.0;
        match self.input.gesture {
            Some(TouchpadGesture::Pan) => {
                if let Interaction::CameraPan { output, camera, .. } = self.input.interaction {
                    let _ = self.pan_step(proof, output, camera, delta, out);
                }
            }
            Some(TouchpadGesture::FourFingerSwipe {
                mut dx,
                mut dy,
                mut triggered,
            }) => {
                dx += delta.0;
                dy += delta.1;
                if !triggered
                    && -dy >= SWIPE_OVERVIEW_THRESHOLD
                    && -dy > dx.abs()
                    && let Some(p) = proof
                {
                    triggered = true;
                    self.overview_toggle(p, out);
                }
                self.input.gesture = Some(TouchpadGesture::FourFingerSwipe { dx, dy, triggered });
            }
            Some(TouchpadGesture::Client(GestureKind::Swipe)) => {
                out.push(Effect::GestureSwipeUpdate { delta, time_ms });
            }
            _ => {}
        }
    }

    pub(super) fn gesture_swipe_end(
        &mut self,
        cancelled: bool,
        time_ms: u32,
        out: &mut Vec<Effect>,
    ) {
        match self.input.gesture {
            Some(TouchpadGesture::Pan) => {
                self.input.gesture = None;
                out.extend(self.end_interaction());
            }
            Some(TouchpadGesture::FourFingerSwipe { .. }) => {
                self.input.gesture = None;
            }
            Some(TouchpadGesture::Client(GestureKind::Swipe)) => {
                self.input.gesture = None;
                out.push(Effect::GestureSwipeEnd { cancelled, time_ms });
            }
            _ => {}
        }
    }

    pub(super) fn gesture_pinch_begin(
        &mut self,
        fingers: u32,
        time_ms: u32,
        out: &mut Vec<Effect>,
    ) {
        if self.input.gesture.is_some()
            || self.input.interaction.active()
            || self.pointer == PointerState::NoOutput
        {
            return;
        }
        if self.touchpad_gestures && fingers == 4 && !self.input.ownership.client_owns_any() {
            self.input.gesture = Some(TouchpadGesture::Pinch { prev_scale: 1.0 });
            return;
        }
        self.input.gesture = Some(TouchpadGesture::Client(GestureKind::Pinch));
        out.push(Effect::GesturePinchBegin { fingers, time_ms });
    }

    pub(super) fn gesture_pinch_update(
        &mut self,
        delta: (f64, f64),
        scale: f64,
        rotation: f64,
        time_ms: u32,
        proof: Option<authority::Authorized>,
        out: &mut Vec<Effect>,
    ) {
        match self.input.gesture {
            Some(TouchpadGesture::Pinch { prev_scale }) => {
                if scale > 0.0 && prev_scale > 0.0 {
                    let factor = scale / prev_scale;
                    self.input.gesture = Some(TouchpadGesture::Pinch { prev_scale: scale });
                    if (factor - 1.0).abs() > 1e-9
                        && let Some(p) = proof
                    {
                        self.zoom_by_factor(p, factor, out);
                    }
                }
            }
            Some(TouchpadGesture::Client(GestureKind::Pinch)) => {
                out.push(Effect::GesturePinchUpdate {
                    delta,
                    scale,
                    rotation,
                    time_ms,
                });
            }
            _ => {}
        }
    }

    pub(super) fn gesture_pinch_end(
        &mut self,
        cancelled: bool,
        time_ms: u32,
        out: &mut Vec<Effect>,
    ) {
        match self.input.gesture {
            Some(TouchpadGesture::Pinch { .. }) => {
                self.input.gesture = None;
            }
            Some(TouchpadGesture::Client(GestureKind::Pinch)) => {
                self.input.gesture = None;
                out.push(Effect::GesturePinchEnd { cancelled, time_ms });
            }
            _ => {}
        }
    }

    pub(super) fn gesture_hold_begin(&mut self, fingers: u32, time_ms: u32, out: &mut Vec<Effect>) {
        if self.input.gesture.is_some()
            || self.input.interaction.active()
            || self.pointer == PointerState::NoOutput
        {
            return;
        }
        self.input.gesture = Some(TouchpadGesture::Client(GestureKind::Hold));
        out.push(Effect::GestureHoldBegin { fingers, time_ms });
    }

    pub(super) fn gesture_hold_end(
        &mut self,
        cancelled: bool,
        time_ms: u32,
        out: &mut Vec<Effect>,
    ) {
        if matches!(
            self.input.gesture,
            Some(TouchpadGesture::Client(GestureKind::Hold))
        ) {
            self.input.gesture = None;
            out.push(Effect::GestureHoldEnd { cancelled, time_ms });
        }
    }
}
