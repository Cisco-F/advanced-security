use defmt::warn;

use crate::drivers::usb_msc::scsi::CBW_SIGNATURE;

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