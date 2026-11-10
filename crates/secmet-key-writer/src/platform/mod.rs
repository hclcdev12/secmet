//! Couche système : énumération des disques, ouverture exclusive, ioctl,
//! élévation de privilèges. Une implémentation par système.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
