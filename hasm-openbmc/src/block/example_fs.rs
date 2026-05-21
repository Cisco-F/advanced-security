//! Synthetic FAT16 block device used by examples and USB MSC smoke tests.
//!
//! The device exposes a tiny, mostly empty 256 MiB FAT16-like disk with one file:
//! `HELLO.TXT`. It is not meant to be a general filesystem implementation; it is
//! a deterministic set of sectors that lets the USB MSC stack be tested without
//! an SD card or remote image server.
//!
//! Important sector layout:
//! - LBA 0: boot sector / BIOS parameter block;
//! - LBA 1 and 257: FAT copies;
//! - LBA 513: root directory sector;
//! - LBA 545: file data for `HELLO.TXT`;
//! - all other sectors: zero-filled.
//!
//! The sparse layout mirrors the FAT16 parameters in the boot sector. Returning
//! zeros for unused sectors makes host filesystem probes see a clean disk instead
//! of random memory.

use crate::block::BlockDevice;

/// Boot sector generated at compile time.
pub static BOOT_SECTOR: [u8; 512] = build_boot_sector();

/// Stateless example backend; all data is derived from static sectors.
pub struct ExampleBlockDevice;

impl BlockDevice for ExampleBlockDevice {
    async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        // Serve only the sectors that contain filesystem metadata or file data.
        match lba {
            0 => buf.copy_from_slice(&BOOT_SECTOR[..Self::BLOCK_SIZE as usize]),
            1 | 257 => buf.copy_from_slice(&FAT_SECTOR[..Self::BLOCK_SIZE as usize]),
            513 => buf.copy_from_slice(&ROOT_DIR_SECTOR[..Self::BLOCK_SIZE as usize]),
            545 => buf.copy_from_slice(&HELLO_DATA_SECTOR[..Self::BLOCK_SIZE as usize]),
            _ => buf.fill(0), // Unused sectors are zero-filled to emulate a clean disk.
        }

        Ok(())
    }

    async fn read_blocks(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        // Keep the example backend simple by delegating multi-sector reads to the
        // single-sector implementation. This also exercises the same code path a
        // host uses for non-sequential reads.
        let blocks_to_read = (buf.len() as u32 + Self::BLOCK_SIZE - 1) / Self::BLOCK_SIZE;
        for i in 0..blocks_to_read {
            let block_lba = lba + i;
            let block_offset = (i * Self::BLOCK_SIZE) as usize;
            let block_buf = &mut buf[block_offset..block_offset + Self::BLOCK_SIZE as usize];
            self.read_block(block_lba, block_buf).await?;
        }
        Ok(())
    }
}

/// Build a FAT16 boot sector / BIOS parameter block.
///
/// The values here advertise a 256 MiB fixed disk with 512-byte sectors and
/// 8 KiB clusters. They are enough for desktop OSes to mount the synthetic disk
/// and locate the single root-directory file.
pub const fn build_boot_sector() -> [u8; 512] {
    let mut buf = [0x00; 512];
    buf[0] = 0xEB; buf[1] = 0x3C; buf[2] = 0x90; 
    // "MSDOS5.0"
    buf[3] = b'M'; buf[4] = b'S'; buf[5] = b'D'; buf[6] = b'O'; buf[7] = b'S'; 
    buf[8] = b'5'; buf[9] = b'.'; buf[10] = b'0';
    
    // Bytes per sector: 512.
    buf[11] = 0x00; buf[12] = 0x02;
    // Sectors per cluster: 16 sectors, or 8 KiB, to fit a 256 MiB FAT16 disk.
    buf[13] = 0x10;
    // One reserved sector: the boot sector itself.
    buf[14] = 0x01; buf[15] = 0x00;
    // Two FAT copies.
    buf[16] = 0x02;
    // Root directory entries: 512 entries, occupying 32 sectors.
    buf[17] = 0x00; buf[18] = 0x02;
    // 16-bit total-sector count must be zero because 524,288 exceeds 65,535.
    buf[19] = 0x00; buf[20] = 0x00;
    // Media descriptor: 0xF8 means fixed disk.
    buf[21] = 0xF8;
    // Sectors per FAT: 256 sectors, enough for the FAT entries.
    buf[22] = 0x00; buf[23] = 0x01; 
    
    // CHS geometry fields are left as zeroes.

    // 32-bit total-sector count at offset 0x20. 524,288 = 0x00080000.
    buf[32] = 0x00; buf[33] = 0x00; buf[34] = 0x08; buf[35] = 0x00;

    // Drive number: 0x80 for a fixed disk.
    buf[36] = 0x80;
    // Extended boot signature.
    buf[38] = 0x29;
    // Volume label: "RUST DISK  ".
    buf[43] = b'R'; buf[44] = b'U'; buf[45] = b'S'; buf[46] = b'T'; buf[47] = b' '; 
    buf[48] = b'D'; buf[49] = b'I'; buf[50] = b'S'; buf[51] = b'K'; buf[52] = b' '; buf[53] = b' ';
    // Filesystem type string: "FAT16   ".
    buf[54] = b'F'; buf[55] = b'A'; buf[56] = b'T'; buf[57] = b'1'; buf[58] = b'6'; 
    buf[59] = b' '; buf[60] = b' '; buf[61] = b' ';
    
    // Boot-sector magic.
    buf[510] = 0x55; buf[511] = 0xAA;
    
    buf
}

// FAT table. FAT16 entries are 16-bit little-endian values.
// FAT16 stores cluster chains as 16-bit little-endian entries. Cluster 2 is the
// first usable data cluster and is marked end-of-chain for HELLO.TXT.
pub static FAT_SECTOR: [u8; 512] = {
    let mut buf = [0; 512];
    // Cluster 0: media type (0xF8) plus reserved end marker.
    buf[0] = 0xF8; buf[1] = 0xFF; 
    // Cluster 1: reserved system marker.
    buf[2] = 0xFF; buf[3] = 0xFF;
    // Cluster 2 contains the file and is marked end-of-chain.
    buf[4] = 0xFF; buf[5] = 0xFF; 
    buf
};

// Root directory. The file still points at cluster 2.
// Root directory entry for `HELLO.TXT`. FAT short names use an 8-byte basename
// plus 3-byte extension, padded with spaces.
pub static ROOT_DIR_SECTOR: [u8; 512] = {
    let mut buf = [0; 512];
    buf[0] = b'H'; buf[1] = b'E'; buf[2] = b'L'; buf[3] = b'L'; buf[4] = b'O'; 
    buf[5] = b' '; buf[6] = b' '; buf[7] = b' '; 
    buf[8] = b'T'; buf[9] = b'X'; buf[10] = b'T'; 
    
    buf[11] = 0x20; 
    
    // Starting cluster number: cluster 2 at FAT16 offset 0x1A.
    buf[0x1A] = 0x02; 
    buf[0x1B] = 0x00;
    // File size in bytes.
    buf[0x1C] = 29; 
    buf
};

// File text contents.
// Data cluster referenced by the root directory entry above.
pub static HELLO_DATA_SECTOR: [u8; 512] = {
    let mut buf = [0; 512];
    let text = b"Welcome to 256MB Rust Disk!\r\n";
    let mut i = 0;
    while i < text.len() {
        buf[i] = text[i];
        i += 1;
    }
    buf
};
