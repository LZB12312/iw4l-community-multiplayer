//! Source-backed constant and clock-derived MotionGraph intent behaviours.
//!
//! These are the small stateful operation boundaries used by the stock graph
//! host. They deliberately return the existing `IntentMutation` protocol so
//! the host remains the sole owner of the intent map.

use crate::input::graph_intents::IntentMutation;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstMgIntent {
    pub value: f32,
    pub on_update: bool,
    created: bool,
}

impl ConstMgIntent {
    pub const fn new(value: f32, on_update: bool) -> Self {
        Self {
            value,
            on_update,
            created: false,
        }
    }

    /// Begin publishes the authored constant and marks the instance alive.
    pub fn begin(&mut self) -> IntentMutation {
        self.created = true;
        IntentMutation::Set(self.value)
    }

    /// TU3 does not republish the constant when `onUpdate` is true. It keeps
    /// the existing map value. A false `onUpdate` instance removes its value
    /// only after the begin frame has elapsed.
    pub fn update(&mut self) -> IntentMutation {
        let mutation = if self.on_update || self.created {
            IntentMutation::None
        } else {
            IntentMutation::Remove
        };
        self.created = false;
        mutation
    }

    /// End removes the owned intent unconditionally. Native End only emits
    /// the remove operation; instance storage is released by the host.
    pub const fn end(&self) -> IntentMutation {
        IntentMutation::Remove
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimeMgIntent {
    pub elapsed: f32,
}

impl TimeMgIntent {
    /// The AG lookup is an existence check only. Any present value advances
    /// the instance clock by the caller's dt; its amplitude is not consumed.
    /// An absent AG key resets the clock and removes the MG entry.
    pub fn update(&mut self, action_value: Option<f32>, dt: f32) -> IntentMutation {
        match action_value {
            Some(_) => {
                self.elapsed += dt;
                IntentMutation::Set(self.elapsed)
            }
            None => {
                self.elapsed = 0.0;
                IntentMutation::Remove
            }
        }
    }

    /// Native End removes the map entry and does not reset elapsed state.
    pub const fn end(&self) -> IntentMutation {
        IntentMutation::Remove
    }
}
