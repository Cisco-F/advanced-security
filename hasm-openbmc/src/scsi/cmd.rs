use defmt::*;
use crate::scsi::{PeripheralType, ScsiResponse, ScsiResponseFormat, ScsiVersion, consts::{MSC_PRODUCT_NAME, MSC_PRODUCT_REVISION, MSC_VENDOR_NAME, SECTOR_COUNT, SECTOR_SIZE, ScsiStatus}};

pub(crate) fn request_sense(buf: &mut [u8]) -> ScsiResponse {
    debug!("SCSI REQUEST SENSE");
    let mut sense = [0u8; 18];
    sense[0] = 0x70; // Current errors
    sense[2] = 0x00; // No sense
    sense[7] = 10; // Additional sense length

    let len = core::cmp::min(sense.len(), buf.len());
    buf[..len].copy_from_slice(&sense[..len]);
    ScsiResponse {
        status: ScsiStatus::ScsiSuccess,
        residue: (sense.len() as u32).saturating_sub(len as u32),
        resp_len: len
    }
}

pub(crate) fn inquiry(buf: &mut [u8]) -> ScsiResponse {
    debug!("SCSI INQUIRY");
    let mut resp = [0u8; 36];
    resp[0] = PeripheralType::DirectAccess as u8;
    resp[1] = 0x80; // Removable media TODO 封装为统一类型
    resp[2] = ScsiVersion::SCSI2 as u8;
    resp[3] = ScsiResponseFormat::SPC2 as u8;
    resp[4] = 31;

    resp[8..16].copy_from_slice(MSC_VENDOR_NAME);
    resp[16..32].copy_from_slice(MSC_PRODUCT_NAME);
    resp[32..36].copy_from_slice(MSC_PRODUCT_REVISION);

    let len = core::cmp::min(resp.len(), buf.len());
    buf[..len].copy_from_slice(&resp[..len]);
    ScsiResponse {
        status: ScsiStatus::ScsiSuccess,
        residue: (resp.len() as u32).saturating_sub(len as u32),
        resp_len: len,
    }
}

pub(crate) fn mode_sense_6(buf: &mut [u8]) -> ScsiResponse {
    debug!("SCSI MODE SENSE(6)");
    let mut resp = [0u8; 4];
    resp[0] = 0x03;
    resp[1] = PeripheralType::DirectAccess as u8;
    resp[2] = 0x00; // Write protected TODO 封装为统一类型
    resp[3] = 0x00; // TODO 块描述符

    let len = core::cmp::min(resp.len(), buf.len());
    buf[..len].copy_from_slice(&resp[..len]);
    ScsiResponse {
        status: ScsiStatus::ScsiSuccess,
        residue: (resp.len() as u32).saturating_sub(len as u32),
        resp_len: len
    }
}

/// 返回设备支持的容量格式列表
pub(crate) fn read_format_capacities(buf: &mut [u8]) -> ScsiResponse {
    debug!("SCSI READ FORMAT CAPACITIES");
    let mut resp = [0u8; 12];
    resp[3] = 0x08;
    resp[4..8].copy_from_slice(&SECTOR_COUNT.to_be_bytes());
    resp[8] = 0x02; // Formatted media

    let block_len = SECTOR_SIZE.to_be_bytes();
    resp[9] = block_len[1];
    resp[10] = block_len[2];
    resp[11] = block_len[3];

    let len = core::cmp::min(resp.len(), buf.len());
    buf[..len].copy_from_slice(&resp[..len]);
    ScsiResponse {
        status: ScsiStatus::ScsiSuccess,
        residue: (resp.len() as u32).saturating_sub(len as u32),
        resp_len: len
    }
}

/// 返回设备的总容量和每个块的大小
pub(crate) fn read_capacity_10(buf: &mut [u8]) -> ScsiResponse {
    debug!("SCSI READ CAPACITY(10)");
    let mut resp = [0u8; 8];
    let last_lba = SECTOR_COUNT - 1;
    resp[0..4].copy_from_slice(&last_lba.to_be_bytes());
    resp[4..8].copy_from_slice(&SECTOR_SIZE.to_be_bytes());

    let len = core::cmp::min(resp.len(), buf.len());
    buf[..len].copy_from_slice(&resp[..len]);
    ScsiResponse {
        status: ScsiStatus::ScsiSuccess,
        residue: (resp.len() as u32).saturating_sub(len as u32),
        resp_len: len
    }
}

/// 读取数据
/// TODO
pub(crate) fn read_10(_buf: &mut [u8], cmd: &[u8]) -> ScsiResponse {
    let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
    let num_blocks = u16::from_be_bytes([cmd[7], cmd[8]]) as u32;
    let total_bytes = (num_blocks * SECTOR_SIZE) as usize;
    
    info!("→ READ_10 LBA={}, Blocks={}", lba, num_blocks);
    
    // 注意：我们直接返回 total_bytes，不再受制于 data_buf 的大小！
    // 因为如果是 READ_10，我们会在 main 循环里特殊处理，不再读取 data_buf！
    ScsiResponse {
        status: ScsiStatus::ScsiSuccess,
        residue: 0,
        resp_len: total_bytes
    }
}