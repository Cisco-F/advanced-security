use defmt::*;
use crate::scsi::consts::*;
use crate::scsi::ScsiResponse;

#[repr(align(4))]
pub struct AlignedBuffer(pub [u8; DISK_SIZE as usize]);

pub static mut RAM_DISK: AlignedBuffer = AlignedBuffer([0u8; DISK_SIZE as usize]);

pub const fn build_boot_sector() -> [u8; 512] {
    let mut buf = [0x00; 512];
    buf[0] = 0xEB; buf[1] = 0x3C; buf[2] = 0x90; 
    // "MSDOS5.0"
    buf[3] = 0x4D; buf[4] = 0x53; buf[5] = 0x44; buf[6] = 0x4F; buf[7] = 0x53; buf[8] = 0x35; buf[9] = 0x2E; buf[10] = 0x30;
    
    // 每扇区512字节
    buf[11] = 0x00; buf[12] = 0x02;
    // 每簇2个扇区
    buf[13] = 0x02;
    // 1个保留扇区
    buf[14] = 0x01; buf[15] = 0x00;
    // 2个FAT表
    buf[16] = 0x02;
    // 根目录项 112个
    buf[17] = 0x70; buf[18] = 0x00;
    // ★★总扇区数: 720 个 (0x02D0) = 360KB★★
    buf[19] = 0xD0; buf[20] = 0x02;
    // 介质描述符
    buf[21] = 0xFD;
    // 每个FAT占2个扇区
    buf[22] = 0x02; buf[23] = 0x00;
    // 签名
    buf[38] = 0x29;
    // 卷标: "RUST DISK  "
    buf[43] = b'R'; buf[44] = b'U'; buf[45] = b'S'; buf[46] = b'T'; buf[47] = b' '; 
    buf[48] = b'D'; buf[49] = b'I'; buf[50] = b'S'; buf[51] = b'K'; buf[52] = b' '; buf[53] = b' ';
    // 文件系统: "FAT12   "
    buf[54] = b'F'; buf[55] = b'A'; buf[56] = b'T'; buf[57] = b'1'; buf[58] = b'2'; buf[59] = b' '; buf[60] = b' '; buf[61] = b' ';
    
    // ★★魔法结束标志★★
    buf[510] = 0x55; buf[511] = 0xAA;
    
    buf
}
pub static BOOT_SECTOR: [u8; 512] = build_boot_sector();

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
    resp[2] = 0x80; // Write protected TODO 封装为统一类型 此处为只读
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
    // resp[0..4].copy_from_slice(&last_lba.to_be_bytes());
    // resp[4..8].copy_from_slice(&SECTOR_SIZE.to_be_bytes());
    resp = [0x00, 0x00, 0x02, 0xCF, 0x00, 0x00, 0x02, 0x00];

    let len = core::cmp::min(resp.len(), buf.len());
    buf[..len].copy_from_slice(&resp[..len]);
    ScsiResponse {
        status: ScsiStatus::ScsiSuccess,
        residue: (resp.len() as u32).saturating_sub(len as u32),
        resp_len: len
    }
}

/// Read data from usb
/// TODO
pub(crate) fn read_10(_buf: &mut [u8], cmd: &[u8]) -> ScsiResponse {
    let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
    let num_blocks = u16::from_be_bytes([cmd[7], cmd[8]]) as u32;
    let start_offset = lba * SECTOR_SIZE;
    let total_bytes = num_blocks * SECTOR_SIZE;
    let end_offset = start_offset + total_bytes;
    let len = core::cmp::min(total_bytes as usize, _buf.len());

    if end_offset > DISK_SIZE {
        error!("READ_10 out of bounds: LBA={}, Blocks={}", lba, num_blocks);
        return ScsiResponse { status: ScsiStatus::ScsiFail, residue: 0, resp_len: 0 };
    }

    // unsafe {
    //     _buf[..len].copy_from_slice(&RAM_DISK.0[start_offset as usize..start_offset as usize + len]);
    // };
    
    info!("→ READ_10 LBA={}, Blocks={}", lba, num_blocks);
    ScsiResponse {
        status: ScsiStatus::ScsiSuccess,
        // residue: (total_bytes as u32).saturating_sub(len as u32),
        residue: 0,
        // resp_len: len
        resp_len: total_bytes as usize,
    }
}