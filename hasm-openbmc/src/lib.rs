#![no_std]

//! HASM-OpenBMC firmware library.
//!
//! The crate is split along the runtime boundaries of the STM32 based BMC:
//! hardware bring-up, network stack setup, low level drivers, block backends,
//! and network-facing services.
//!
//! The firmware intentionally keeps every long-running feature as an Embassy
//! task. This avoids a central polling loop and lets the UART bridge, Redfish
//! control plane, USB mass-storage emulation, LEDs, and network driver make
//! progress independently.
//!
//! Most services use fixed-size buffers and `static_cell` storage because the
//! target runs in `no_std` mode without a general allocator. When a module owns
//! a static buffer, it should document which peripheral or protocol path uses
//! that memory so buffer pressure is visible during reviews.
//!
//! The main data path is:
//! network image server -> `RemoteBlockDevice` -> `CachedData` -> SCSI READ(10)
//! -> USB MSC bulk endpoint -> Raspberry Pi boot ROM or operating system.
//! Keeping this path read-only is a deliberate safety choice: the controlled
//! Raspberry Pi can boot from the virtual disk without being able to corrupt the
//! backing image on the host.

pub mod hal;
pub mod drivers;
pub mod services;
pub mod config;
pub mod net;
pub mod block;
pub mod utils;
pub mod consts;
