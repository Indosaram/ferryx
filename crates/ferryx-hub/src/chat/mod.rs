pub mod approval;
pub mod discord;
pub mod fence;
pub mod reply;
pub mod router;
pub mod session;
pub mod slack;

#[cfg(feature = "hub-voice")]
pub mod voice {
    //! Voice and huddle are parked. This module exists so the feature compiles and does nothing.
    pub const PARKED: &str = "voice and huddle are not ported";
}
