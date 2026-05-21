//! USB MSC Bulk-Only Transport packet structures.
//!
//! The host wraps every SCSI command in a 31-byte Command Block Wrapper (CBW).
//! After any command data phase, the device answers with a 13-byte Command Status
//! Wrapper (CSW). Both packet types use little-endian integers even though many
//! SCSI command fields inside `cmd` use big-endian byte order.
//!
//! The structures are parsed/serialized manually rather than using unsafe casts.
//! That keeps the code portable across alignment rules and makes the byte layout
//! explicit for protocol review.

use defmt::warn;

use crate::drivers::usb_msc::scsi::{CBW_SIGNATURE, CSW_SIGNATURE};

#[derive(Debug, Clone, Copy)]
#[repr(C, align(4))]
/// Command Block Wrapper sent by the USB host.
pub struct Cbw {
    /// Must be `USBC` in little-endian form.
    pub signature: u32,
    /// Opaque host tag echoed back in the CSW.
    pub tag: u32,
    /// Expected data-transfer length for the command.
    pub dtl: u32,
    /// Direction bit; bit 7 set means device-to-host.
    pub flags: u8,
    /// Length of the SCSI command descriptor block in `cmd`.
    pub cb_len: u8,
    /// SCSI command descriptor block, padded to 16 bytes.
    pub cmd: [u8; 16],
}

impl Cbw {
    /// Parse a CBW from the 31-byte bulk OUT packet.
    pub fn from_bytes(buf: &[u8]) -> Self {
        let signature = u32::from_le_bytes([
            buf[0],
            buf[1],
            buf[2],
            buf[3]
        ]);
        let tag = u32::from_le_bytes([
            buf[4],
            buf[5],
            buf[6],
            buf[7]
        ]);
        let dtl = u32::from_le_bytes([
            buf[8],
            buf[9],
            buf[10],
            buf[11]
        ]);
        let flags = buf[12];
        let cb_len = core::cmp::min(buf[14], 16);
        // Clamp the command length to the fixed 16-byte storage so malformed
        // hosts cannot make the copy exceed the local array.
        let cmd = {
            let mut arr = [0u8; 16];
            arr[..cb_len as usize].copy_from_slice(&buf[15..15 + cb_len as usize]);
            arr
        };

        if signature != CBW_SIGNATURE {
            warn!("Invalid CBW signature: 0x{:08x}", signature);
        }

        Self {
            signature,
            tag,
            dtl,
            flags,
            cb_len,
            cmd
        }
    }
}

/// Command Status Wrapper sent after each SCSI command.
pub struct Csw {
    /// Must be `USBS` in little-endian form.
    pub signature: u32,
    /// Echo of the CBW tag so the host can match command/status pairs.
    pub tag: u32,
    /// Bytes not transferred during the data phase.
    pub residue: u32,
    /// SCSI transport status: 0 success, 1 failed.
    pub status: u8,
}

impl Csw {
    /// Create a status packet for a completed command.
    pub fn new(tag: u32, residue: u32, status: u8) -> Self {
        Self {
            signature: CSW_SIGNATURE,
            tag,
            residue,
            status,
        }
    }

    /// Serialize the CSW into the exact 13-byte wire format.
    pub fn to_bytes(&self) -> [u8; 13] {
        let mut buf = [0u8; 13];
        buf[0..4].copy_from_slice(&self.signature.to_le_bytes());
        buf[4..8].copy_from_slice(&self.tag.to_le_bytes());
        buf[8..12].copy_from_slice(&self.residue.to_le_bytes());
        buf[12] = self.status;
        buf
    }
}
