//! Common block-device abstraction used by all USB MSC storage backends.
//!
//! The USB mass-storage service speaks in 512-byte sectors because that is what
//! Raspberry Pi boot firmware and the SCSI READ(10) command expect. Backends can
//! be a TF card, a remote HTTP image, or an in-memory example filesystem, but
//! they all expose the same sector interface to the SCSI layer.
//!
//! The trait stays deliberately small:
//! - `read_block` is the minimal operation needed by the command loop.
//! - `read_blocks` lets optimized backends fetch adjacent sectors in one pass.
//! - `write_block` defaults to unsupported so the virtual USB disk remains
//!   read-only unless a backend explicitly opts into writes.
//!
//! Returning `Result<(), ()>` keeps call sites compact in `no_std` firmware.
//! Detailed diagnostics are still emitted through `defmt` at the failure point.

use defmt::error;

use crate::drivers::usb_msc::scsi::SECTOR_SIZE;

pub mod example_fs;
pub mod tf;
pub mod remote;
pub mod cached_data;

#[allow(async_fn_in_trait)]
pub trait BlockDevice {
    /// Logical sector size exposed through USB MSC.
    const BLOCK_SIZE: u32 = SECTOR_SIZE;

    /// Read exactly one logical block into `buf`.
    ///
    /// Callers are expected to pass a 512-byte buffer. Implementations that wrap
    /// hardware or network storage should log the failing LBA before returning
    /// `Err(())`, because the error type itself carries no context.
    async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()>;

    /// Read a contiguous run of logical blocks starting at `lba`.
    ///
    /// The buffer length determines how many bytes are requested. Backends may
    /// round or split internally, but the visible contract is that the caller's
    /// buffer is filled with the requested sector data on success.
    async fn read_blocks(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()>;

    /// Optional write path for mutable backends.
    ///
    /// Most boot flows should keep this disabled so the host-side image remains
    /// reproducible across test runs. A TF-card backend can override it when the
    /// firmware is used as a real removable disk rather than a boot image proxy.
    async fn write_block(&mut self, _lba: u32, _buf: &[u8]) -> Result<(), ()> {
        error!("Write operation not supported on this block device");
        Err(())
    }
}
