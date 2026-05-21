//! Hardware driver modules.
//!
//! These modules wrap STM32 peripherals into the higher-level pieces used by the
//! BMC services: Ethernet, UART, TF/SD storage, USB MSC, and the power LED.

pub mod ethernet;
pub mod uart;
pub mod tf;
pub mod usb_msc;
pub mod led;
