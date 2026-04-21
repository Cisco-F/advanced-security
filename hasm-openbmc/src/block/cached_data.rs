use defmt::info;

use crate::block::BlockDevice;


static CACHE_BUF: static_cell::StaticCell<[u8; 24 * 512]> = static_cell::StaticCell::new();

pub struct CachedData<D: BlockDevice> {
    inner: D,
    data: &'static mut [u8; 24 * 512],
    start_lba: u32,
    valid: bool
}

impl<D: BlockDevice> CachedData<D> {
    pub fn new(inner: D) -> Self {
        Self {
            inner,
            data: CACHE_BUF.init([0u8; 24 * 512]),
            start_lba: 0,
            valid: false
        }
    }
}

impl<D: BlockDevice> BlockDevice for CachedData<D> {
    async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        let max_cached_blocks = (self.data.len() / 512) as u32;
        if self.valid &&
           lba >= self.start_lba &&
           lba < self.start_lba + max_cached_blocks {

            let offset = (lba - self.start_lba) as usize;
            buf.copy_from_slice(&self.data[offset * 512..(offset + 1) * 512]);
            return Ok(());
        } 

        // triggers a new cache load
        info!("Cache miss for LBA {}, loading blocks {}-{}", lba, lba, lba + max_cached_blocks - 1);
        self.start_lba = lba;
        self.inner.read_blocks(lba, &mut self.data[..]).await?;
        self.valid = true;
        let offset = (lba - self.start_lba) as usize;
        buf.copy_from_slice(&self.data[offset * 512..(offset + 1) * 512]);
        Ok(())
    }

    async fn read_blocks(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        self.inner.read_blocks(lba, buf).await?;
        Ok(())
    }
}