pub mod cmd;
pub mod consts;


pub use crate::drivers::usb_msc::scsi::consts::*;
use crate::{drivers::usb_msc::{device::ScsiDataSink, scsi::cmd::*, transport::Cbw}, storage::BlockDevice};

#[derive(Debug, Clone, Copy)]
pub struct ScsiResponse {
    pub status: ScsiStatus,
    pub residue: u32,
}

impl ScsiResponse {
    pub fn success() -> Self {
        Self {
            status: ScsiStatus::ScsiSuccess,
            residue: 0,
        }
    }

    pub fn fail(residue: u32) -> Self {
        Self {
            status: ScsiStatus::ScsiFail,
            residue,
        }
    }
}

pub async fn handle_scsi_cmd(
    block_device: &mut impl BlockDevice,
    sink: &mut impl ScsiDataSink,
    cbw: Cbw,
) -> ScsiResponse {
    let cmd = cbw.cmd;
    if cmd.is_empty() { return ScsiResponse::fail(0) }
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