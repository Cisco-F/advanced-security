//! SCSI command handlers for a read-only USB boot disk.
//!
//! Each function writes the exact response payload required by the command and
//! returns a compact `ScsiResponse` used later to build the CSW. The command layer
//! does not own USB endpoints directly; it writes through `ScsiDataSink` so the
//! protocol logic stays separate from the concrete device driver.
//!
//! Multi-byte fields in SCSI responses are generally big-endian. That differs
//! from CBW/CSW transport packets, which are little-endian, so handlers call
//! `to_be_bytes` at the point where the field is placed on the wire.

use crate::drivers::usb_msc::device::ScsiDataSink;
use crate::drivers::usb_msc::scsi::ScsiResponse;
use crate::drivers::usb_msc::scsi::consts::*;
use crate::drivers::usb_msc::transport::Cbw;
use crate::block::BlockDevice;
use defmt::*;

pub(crate) async fn request_sense(sink: &mut impl ScsiDataSink) -> ScsiResponse {
    debug!("SCSI REQUEST SENSE");
    // The current firmware does not track detailed sense data. Returning
    // "current errors / no sense" lets hosts continue normal media probing.
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
    // Standard 36-byte inquiry response. Vendor/product strings come from
    // constants so the advertised identity is consistent across handlers.
    let mut resp = [0u8; 36];
    resp[0] = PeripheralType::DirectAccess as u8;
    resp[1] = 0x80; // Removable media; keep as a raw flag until typed helpers exist.
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
    // Mark the medium as write-protected so operating systems do not attempt to
    // modify the remote boot image.
    let mut resp = [0u8; 4];
    resp[0] = 0x03;
    resp[1] = PeripheralType::DirectAccess as u8;
    resp[2] = 0x80; // Write protected; this virtual disk is intentionally read-only.
    resp[3] = 0x00; // No block descriptors are returned.

    match sink.write(&resp).await {
        Ok(_) => ScsiResponse::success(),
        Err(e) => {
            warn!("Failed to send MODE SENSE(6) response: {:?}", e.usb_error);
            ScsiResponse::fail(e.residue)
        }
    }
}

pub(crate) async fn read_format_capacities(sink: &mut impl ScsiDataSink) -> ScsiResponse {
    debug!("SCSI READ FORMAT CAPACITIES");
    // Advertise a single formatted capacity descriptor matching the virtual
    // disk size. The block length is stored as a 24-bit big-endian value.
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

pub(crate) async fn read_capacity_10(sink: &mut impl ScsiDataSink) -> ScsiResponse {
    debug!("SCSI READ CAPACITY(10)");
    // READ CAPACITY returns the last valid LBA, not the total sector count.
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

pub(crate) async fn read_10<B: BlockDevice>(
    block_device: &mut B,
    sink: &mut impl ScsiDataSink,
    cbw: Cbw,
) -> ScsiResponse {
    let cmd = cbw.cmd;
    // READ(10) uses big-endian LBA and transfer-length fields inside the CDB.
    let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
    let num_blocks = u16::from_be_bytes([cmd[7], cmd[8]]) as u32;
    let start_offset = lba * SECTOR_SIZE;
    let total_bytes = cbw.dtl;
    let end_offset = start_offset + total_bytes;

    if end_offset > DISK_SIZE {
        // Bounds checking protects both local TF storage and remote image
        // serving from malformed host requests.
        error!("READ_10 out of bounds: LBA={}, Blocks={}", lba, num_blocks);
        return ScsiResponse::fail(total_bytes);
    }

    info!("→ READ_10 LBA={}, Blocks={}", lba, num_blocks);

    let mut buf = [0u8; SECTOR_SIZE as usize];
    let mut offset = 0usize;
    for blk in 0..num_blocks {
        // Stream one sector at a time to keep stack use bounded. The cache layer
        // can still batch remote HTTP reads beneath this loop.
        if offset >= total_bytes as usize {
            break;
        }

        if let Err(_) = block_device.read_block(lba + blk, &mut buf).await {
            error!("Block device read error at LBA={}", lba + blk);
            return ScsiResponse::fail(total_bytes);
        }

        let send_len = core::cmp::min(SECTOR_SIZE as usize, (total_bytes as usize) - offset);
        // The final transfer can be shorter than one sector if the host's data
        // transfer length is smaller than the block count implies.
        if let Err(e) = sink.write(&buf[..send_len]).await {
            warn!("Failed to send READ_10 data: {:?}", e.usb_error);
            return ScsiResponse::fail(total_bytes - (blk * SECTOR_SIZE));
        }
        offset += send_len;
    }

    ScsiResponse::success()
}
