pub mod cmd;
pub mod consts;

pub use crate::scsi::consts::*;

#[derive(Debug, Clone, Copy)]
pub struct ScsiResponse {
    pub status: ScsiStatus,
    pub residue: u32,
    pub resp_len: usize,
}

pub fn handle_scsi_cmd(cmd: &[u8], buf: &mut [u8]) -> ScsiResponse {
    if cmd.is_empty() { return ScsiResponse { status: ScsiStatus::ScsiFail, residue: 0, resp_len: 0 }; }
    match cmd[0] {
        SCSI_TEST_UNIT_READY => ScsiResponse { status: ScsiStatus::ScsiSuccess, residue: 0, resp_len: 0 },
        SCSI_REQUEST_SENSE => crate::scsi::cmd::request_sense(buf),
        SCSI_INQUIRY => crate::scsi::cmd::inquiry(buf),
        SCSI_MODE_SENSE_6 => crate::scsi::cmd::mode_sense_6(buf),
        SCSI_READ_FORMAT_CAPACITIES => crate::scsi::cmd::read_format_capacities(buf),
        SCSI_READ_CAPACITY_10 => crate::scsi::cmd::read_capacity_10(buf),
        SCSI_READ_10 => crate::scsi::cmd::read_10(buf, cmd),
        _ => ScsiResponse { status: ScsiStatus::ScsiFail, residue: 0, resp_len: 0 },
    }
}