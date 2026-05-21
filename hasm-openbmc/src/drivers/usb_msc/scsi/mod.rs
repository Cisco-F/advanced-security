//! SCSI command dispatcher for the USB MSC device.
//!
//! The Raspberry Pi boot ROM and common operating systems need only a small
//! subset of SCSI transparent commands for read-only boot media. This dispatcher
//! keeps that subset explicit and routes each command to a handler that knows how
//! to format its response bytes.
//!
//! Unsupported commands currently fail with a generic CHECK CONDITION-style CSW.
//! That is sufficient for host probing because clients typically fall back to
//! REQUEST SENSE, INQUIRY, capacity reads, and READ(10).

pub mod cmd;
pub mod consts;

pub use crate::drivers::usb_msc::scsi::consts::*;
use crate::{drivers::usb_msc::{device::ScsiDataSink, scsi::cmd::*, transport::Cbw}, block::BlockDevice};

#[derive(Debug, Clone, Copy)]
/// Result of a SCSI command as needed by the USB MSC CSW.
pub struct ScsiResponse {
    /// Transport-level SCSI status byte.
    pub status: ScsiStatus,
    /// Number of bytes that were not transferred.
    pub residue: u32,
}

impl ScsiResponse {
    /// Successful command with no residue.
    pub fn success() -> Self {
        Self {
            status: ScsiStatus::ScsiSuccess,
            residue: 0,
        }
    }

    /// Failed command carrying the untransferred byte count.
    pub fn fail(residue: u32) -> Self {
        Self {
            status: ScsiStatus::ScsiFail,
            residue,
        }
    }
}

/// Decode the SCSI opcode in `cbw` and run the matching handler.
pub async fn handle_scsi_cmd(
    block_device: &mut impl BlockDevice,
    sink: &mut impl ScsiDataSink,
    cbw: Cbw,
) -> ScsiResponse {
    let cmd = cbw.cmd;
    if cmd.is_empty() { return ScsiResponse::fail(0) }
    // The first byte of a SCSI command descriptor block is the operation code.
    match cmd[0] {
        SCSI_TEST_UNIT_READY => ScsiResponse::success(),
        SCSI_REQUEST_SENSE => request_sense(sink).await,
        SCSI_INQUIRY => inquiry(sink).await,
        SCSI_MODE_SENSE_6 => mode_sense_6(sink).await,
        SCSI_READ_FORMAT_CAPACITIES => read_format_capacities(sink).await,
        SCSI_READ_CAPACITY_10 => read_capacity_10(sink).await,
        SCSI_READ_10 => read_10(block_device, sink, cbw).await,
        _ => ScsiResponse::fail(0),
    }
}
