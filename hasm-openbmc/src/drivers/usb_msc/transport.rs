use defmt::warn;

use crate::drivers::usb_msc::scsi::{CBW_SIGNATURE, CSW_SIGNATURE};

#[derive(Debug, Clone, Copy)]
#[repr(C, align(4))]
pub struct Cbw {
    pub signature: u32,
    pub tag: u32,
    pub dtl: u32,
    pub flags: u8,
    pub cb_len: u8,
    pub cmd: [u8; 16],
}

impl Cbw {
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

pub struct Csw {
    pub signature: u32,
    pub tag: u32,
    pub residue: u32,
    pub status: u8,
}

impl Csw {
    pub fn new(tag: u32, residue: u32, status: u8) -> Self {
        Self {
            signature: CSW_SIGNATURE,
            tag,
            residue,
            status,
        }
    }

    pub fn to_bytes(&self) -> [u8; 13] {
        let mut buf = [0u8; 13];
        buf[0..4].copy_from_slice(&self.signature.to_le_bytes());
        buf[4..8].copy_from_slice(&self.tag.to_le_bytes());
        buf[8..12].copy_from_slice(&self.residue.to_le_bytes());
        buf[12] = self.status;
        buf
    }
}