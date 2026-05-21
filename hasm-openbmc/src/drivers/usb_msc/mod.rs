//! USB Mass Storage Class support.
//!
//! `device` owns descriptors and endpoints, `transport` handles CBW/CSW packet
//! layout, and `scsi` implements the small read-only command set used for boot.

pub mod device;
pub mod transport;
pub mod scsi;
