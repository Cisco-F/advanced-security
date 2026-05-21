//! Long-running Embassy service tasks.
//!
//! Services expose the firmware's user-facing behavior: power control, virtual
//! USB storage, management HTTP, the optional VNC diagnostic endpoint, and the
//! TCP UART console bridge.

pub mod power_control;
pub mod virtual_usb;
pub mod web_server;
pub mod vnc_server;
pub mod console;
