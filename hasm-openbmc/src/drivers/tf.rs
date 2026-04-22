use defmt::{error, info};
use embassy_stm32::{
    Peri, bind_interrupts,
    peripherals::{self, DMA2_CH3, PC8, PC9, PC10, PC11, PC12, PD2, SDIO},
    sdmmc::{self, DataBlock, Sdmmc},
    time::Hertz,
};

use crate::drivers::usb_msc::scsi::SECTOR_SIZE;

bind_interrupts!(struct TfIrqs {
    SDIO => sdmmc::InterruptHandler<peripherals::SDIO>;
});

pub struct TfCard {
    sdmmc: Option<Sdmmc<'static, peripherals::SDIO>>,
}

impl TfCard {
    pub fn new() -> Self {
        Self { sdmmc: None }
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
        let mut sdmmc = Sdmmc::new_4bit(
            sdmmc,
            TfIrqs,
            dma,
            clk,
            cmd,
            d0,
            d1,
            d2,
            d3,
            Default::default(),
        );

        match &sdmmc.init_sd_card(Hertz(400_000)).await {
            Ok(_) => info!("TF Card init OK"),
            Err(e) => {
                error!("TF Card init failed: {:?}", e);
                return Err(());
            }
        }

        if let Ok(card) = sdmmc.card() {
            match card {
                sdmmc::SdmmcPeripheral::SdCard(c) => {
                    info!("SD Card detected, CSD version: {}", c.csd.version())
                }
                sdmmc::SdmmcPeripheral::Emmc(_) => info!("eMMC detected"),
            }
        }

        self.sdmmc = Some(sdmmc);
        Ok(())
    }

    pub async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        let mut data_block = DataBlock([0u8; SECTOR_SIZE as usize]);
        match self
            .sdmmc
            .as_mut()
            .unwrap()
            .read_block(lba, &mut data_block)
            .await
        {
            Ok(_) => {
                buf.copy_from_slice(&data_block.0);
                return Ok(());
            }
            Err(e) => {
                error!("Failed to read block {}: {:?}", lba, e);
                return Err(());
            }
        }
    }

    pub async fn write_block(&mut self, lba: u32, buf: &[u8]) -> Result<(), ()> {
        let mut data_block = DataBlock([0u8; SECTOR_SIZE as usize]);
        data_block.0.copy_from_slice(buf);
        match self
            .sdmmc
            .as_mut()
            .unwrap()
            .write_block(lba, &data_block)
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Failed to write block {}: {:?}", lba, e);
                Err(())
            }
        }
    }
}
