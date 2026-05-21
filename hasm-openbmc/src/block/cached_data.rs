//! Small read-through cache for block backends.
//!
//! Raspberry Pi boot and OS probing tend to read nearby sectors repeatedly:
//! partition tables, FAT metadata, boot files, and filesystem headers are often
//! revisited in short bursts. Fetching those sectors one-by-one from the remote
//! HTTP image server would create many TCP connections and visible boot latency.
//!
//! This cache grabs 24 sectors at a time, stores them in a single static buffer,
//! and serves subsequent `read_block` calls directly while the requested LBA is
//! still inside that window.
//!
//! The cache is intentionally simple:
//! - one window, no eviction policy beyond replacement on miss;
//! - no write-back path, because the exported USB disk is read-only;
//! - static storage, because the firmware has no heap allocator.
//!
//! `read_blocks` bypasses the cache. Large contiguous transfers already give the
//! underlying backend enough context to optimize the request.

use defmt::info;

use crate::block::BlockDevice;

static CACHE_BUF: static_cell::StaticCell<[u8; 24 * 512]> = static_cell::StaticCell::new();

/// Read-through cache wrapper for any `BlockDevice`.
pub struct CachedData<D: BlockDevice> {
    /// Backend that actually owns the storage or network connection.
    inner: D,
    /// Cached sector data for the current window.
    data: &'static mut [u8; 24 * 512],
    /// First LBA represented by `data`.
    start_lba: u32,
    /// Whether `data` contains a loaded window.
    valid: bool
}

impl<D: BlockDevice> CachedData<D> {
    /// Create a cache around `inner` and allocate its static sector window.
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
        // Fast path: the requested sector falls inside the current cache
        // window, so no backend I/O or TCP connection is needed.
        if self.valid &&
           lba >= self.start_lba &&
           lba < self.start_lba + max_cached_blocks {

            let offset = (lba - self.start_lba) as usize;
            buf.copy_from_slice(&self.data[offset * 512..(offset + 1) * 512]);
            return Ok(());
        } 

        // triggers a new cache load
        // On a miss, align the new cache window exactly at the requested LBA.
        // This favors forward sequential reads, which are the common case while
        // firmware and bootloaders scan the virtual USB disk.
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
