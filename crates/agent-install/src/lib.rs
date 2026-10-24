// The installer is used only by the macOS desktop host. Unix support also lets
// its filesystem state machine run against isolated directories on CI.
#[cfg(unix)]
mod installer;
#[cfg(unix)]
pub use installer::*;
