use crate::drivers::usb_msc::device::ScsiDataSink;
use crate::drivers::usb_msc::scsi::ScsiResponse;
use crate::drivers::usb_msc::scsi::consts::*;
use crate::drivers::usb_msc::transport::Cbw;
use crate::block::BlockDevice;
use defmt::*;

pub(crate) async fn request_sense(sink: &mut impl ScsiDataSink) -> ScsiResponse {
    debug!("SCSI REQUEST SENSE");
    let mut sense = [0u8; 18];
    sense[0] = 0x70; // Current errors
    sense[2] = 0x00; // No sense
    sense[7] = 10; // Additional sense length

    match sink.write(&sense).await {
        Ok(_) => ScsiResponse::success(),
        Err(e) => {
            warn!("Failed to send REQUEST SENSE response: {:?}", e.usb_error);
            ScsiResponse::fail(e.residue)
        }
    }
}

pub(crate) async fn inquiry(sink: &mut impl ScsiDataSink) -> ScsiResponse {
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

    match sink.write(&resp).await {
        Ok(_) => ScsiResponse::success(),
        Err(e) => {
            warn!("Failed to send REQUEST SENSE response: {:?}", e.usb_error);
            ScsiResponse::fail(e.residue)
        }
    }
}

pub(crate) async fn mode_sense_6(sink: &mut impl ScsiDataSink) -> ScsiResponse {
    debug!("SCSI MODE SENSE(6)");
    let mut resp = [0u8; 4];
    resp[0] = 0x03;
    resp[1] = PeripheralType::DirectAccess as u8;
    resp[2] = 0x80; // Write protected TODO 封装为统一类型 此处为只读
    resp[3] = 0x00; // TODO 块描述符

    match sink.write(&resp).await {
        Ok(_) => ScsiResponse::success(),
        Err(e) => {
            warn!("Failed to send MODE SENSE(6) response: {:?}", e.usb_error);
            ScsiResponse::fail(e.residue)
        }
    }
}

/// 返回设备支持的容量格式列表
pub(crate) async fn read_format_capacities(sink: &mut impl ScsiDataSink) -> ScsiResponse {
    debug!("SCSI READ FORMAT CAPACITIES");
    let mut resp = [0u8; 12];
    resp[3] = 0x08;
    resp[4..8].copy_from_slice(&SECTOR_COUNT.to_be_bytes());
    resp[8] = 0x02; // Formatted media

    let block_len = SECTOR_SIZE.to_be_bytes();
    resp[9] = block_len[1];
    resp[10] = block_len[2];
    resp[11] = block_len[3];

    match sink.write(&resp).await {
        Ok(_) => ScsiResponse::success(),
        Err(e) => {
            warn!(
                "Failed to send READ FORMAT CAPACITIES response: {:?}",
                e.usb_error
            );
            ScsiResponse::fail(e.residue)
        }
    }
}

/// 返回设备的总容量和每个块的大小
pub(crate) async fn read_capacity_10(sink: &mut impl ScsiDataSink) -> ScsiResponse {
    debug!("SCSI READ CAPACITY(10)");
    let mut resp = [0u8; 8];
    let last_lba = SECTOR_COUNT - 1;
    resp[0..4].copy_from_slice(&last_lba.to_be_bytes());
    resp[4..8].copy_from_slice(&SECTOR_SIZE.to_be_bytes());
    // resp = [0x00, 0x00, 0x02, 0xCF, 0x00, 0x00, 0x02, 0x00];

    match sink.write(&resp).await {
        Ok(_) => ScsiResponse::success(),
        Err(e) => {
            warn!(
                "Failed to send READ CAPACITY(10) response: {:?}",
                e.usb_error
            );
            ScsiResponse::fail(e.residue)
        }
    }
}

/// Read data from usb
/// TODO
pub(crate) async fn read_10<B: BlockDevice>(
    block_device: &mut B,
    sink: &mut impl ScsiDataSink,
    cbw: Cbw,
) -> ScsiResponse {
    let cmd = cbw.cmd;
    let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
    let num_blocks = u16::from_be_bytes([cmd[7], cmd[8]]) as u32;
    let start_offset = lba * SECTOR_SIZE;
    let total_bytes = cbw.dtl;
    let end_offset = start_offset + total_bytes;

    if end_offset > DISK_SIZE {
        error!("READ_10 out of bounds: LBA={}, Blocks={}", lba, num_blocks);
        return ScsiResponse::fail(total_bytes);
    }

    info!("→ READ_10 LBA={}, Blocks={}", lba, num_blocks);

    let mut buf = [0u8; SECTOR_SIZE as usize];
    let mut offset = 0usize;
    for blk in 0..num_blocks {
        if offset >= total_bytes as usize {
            break;
        }

        if let Err(_) = block_device.read_block(lba + blk, &mut buf).await {
            error!("Block device read error at LBA={}", lba + blk);
            return ScsiResponse::fail(total_bytes);
        }

        let send_len = core::cmp::min(SECTOR_SIZE as usize, (total_bytes as usize) - offset);
        if let Err(e) = sink.write(&buf[..send_len]).await {
            warn!("Failed to send READ_10 data: {:?}", e.usb_error);
            return ScsiResponse::fail(total_bytes - (blk * SECTOR_SIZE));
        }
        offset += send_len;
    }

    ScsiResponse::success()
}
