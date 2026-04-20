use defmt::{error, info};
use embassy_stm32::{
	Peri, peripherals::{DMA2_CH3, PC8, PC9, PC10, PC11, PC12, PD2, SDIO}
};

use crate::{drivers::tf::TfCard, storage::BlockDevice};


pub struct TfBlockDevice<'d> {
	inner: TfCard<'d>,
}

impl<'d> TfBlockDevice<'d> {
	pub fn new() -> Self {
		Self { inner: TfCard::new() }
	}

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

impl<'d> BlockDevice for TfBlockDevice<'d> {
	async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
		self.inner.read_block(lba, buf).await?;
		Ok(())
	}

	async fn write_block(&mut self, lba: u32, buf: &[u8]) -> Result<(), ()> {
		self.inner.write_block(lba, buf).await?;
		Ok(())
	}
}