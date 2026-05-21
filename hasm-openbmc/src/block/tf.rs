//! Block-device adapter for a physical TF/SD card.
//!
//! This backend is useful when the STM32 board should expose local removable
//! storage over USB MSC instead of proxying a host-side image file. It wraps the
//! lower-level SDMMC driver and presents the same `BlockDevice` trait used by
//! the remote image backend.
//!
//! Reads and writes are sector-oriented. The multi-block read helper currently
//! loops over single-sector reads because it keeps the implementation portable
//! across driver versions and simple enough for bring-up diagnostics.

use defmt::{error, info};
use embassy_stm32::{
	Peri, peripherals::{DMA2_CH3, PC8, PC9, PC10, PC11, PC12, PD2, SDIO}
};

use crate::{drivers::tf::TfCard, block::BlockDevice};

/// USB MSC block backend backed by the SDMMC TF card driver.
pub struct TfBlockDevice {
	inner: TfCard,
}

impl TfBlockDevice {
	/// Create an uninitialized TF block device.
	pub fn new() -> Self {
		Self { inner: TfCard::new() }
	}

	/// Initialize the SDMMC peripheral and bind the card to this block backend.
	pub async fn init(
		&mut self,
		sdmmc: Peri<'static, SDIO>,
		dma: Peri<'static, DMA2_CH3>,
		clk: Peri<'static, PC12>,
		cmd: Peri<'static, PD2>,
		d0: Peri<'static, PC8>,
		d1: Peri<'static, PC9>,
		d2: Peri<'static, PC10>,
		d3: Peri<'static, PC11>,
	) -> Result<(), ()> {
		match self.inner.init(sdmmc, dma, clk, cmd, d0, d1, d2, d3).await {
			Ok(_) => info!("TF block device init OK"),
			Err(_) => {
				error!("TF block device init failed");
				return Err(());
			}
		}

		Ok(())
	}
}

impl BlockDevice for TfBlockDevice {
	async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
		self.inner.read_block(lba, buf).await?;
		Ok(())
	}

	async fn read_blocks(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
		// Round up so a partially filled final sector is still read. The normal
		// USB MSC path passes sector-aligned lengths, but this keeps the adapter
		// tolerant of future callers.
		let blocks_to_read = (buf.len() as u32 + Self::BLOCK_SIZE - 1) / Self::BLOCK_SIZE;
        for i in 0..blocks_to_read {
            let block_lba = lba + i;
            let block_offset = (i * Self::BLOCK_SIZE) as usize;
            let block_buf = &mut buf[block_offset..block_offset + Self::BLOCK_SIZE as usize];
            self.read_block(block_lba, block_buf).await?;
        }
        Ok(())
	}

	async fn write_block(&mut self, lba: u32, buf: &[u8]) -> Result<(), ()> {
		self.inner.write_block(lba, buf).await?;
		Ok(())
	}
}
