// Each platform gets its own backend; all of them emit the same AppEvents.
#[cfg(target_os = "linux")]
pub mod bluetooth;
#[cfg(target_os = "macos")]
#[path = "bluetooth_macos.rs"]
pub mod bluetooth;
pub mod subghz;
pub mod wifi;
