use defmt::error;

use crate::drivers::usb_msc::scsi::SECTOR_SIZE;


pub mod example_fs;

#[allow(async_fn_in_trait)]
pub trait BlockDevice {
    const BLOCK_SIZE: u32 = SECTOR_SIZE;

    async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()>;

    async fn write_block(&mut self, _lba: u32, _buf: &[u8]) -> Result<(), ()> {
        error!("Write operation not supported on this block device");
        Err(())
    }
}