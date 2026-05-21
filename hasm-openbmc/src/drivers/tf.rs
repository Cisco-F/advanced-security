//! Low-level TF/SD card driver wrapper.
//!
//! The STM32F407 SDIO peripheral is configured in 4-bit mode for better transfer
//! speed than SPI-style access. This module owns the concrete pins and DMA
//! channel, while `block::tf` adapts the card to the generic USB MSC block
//! interface.
//!
//! Initialization starts at 400 kHz, matching SD card identification timing.
//! After Embassy completes card setup, later transfers are managed by the driver.
//!
//! The wrapper stores the `Sdmmc` object in an `Option` so construction and
//! asynchronous initialization can be separate steps. Callers should initialize
//! before reading or writing; current methods unwrap to surface misuse quickly
//! during firmware bring-up.

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

/// Owns the SDMMC peripheral after successful TF card initialization.
pub struct TfCard {
    sdmmc: Option<Sdmmc<'static, peripherals::SDIO>>,
}

impl TfCard {
    /// Construct an empty card wrapper; call `init` before I/O.
    pub fn new() -> Self {
        Self { sdmmc: None }
    }

    /// Initialize the SDIO bus, identify the card, and retain the driver.
    ///
    /// The pin list mirrors the 4-bit SDIO bus: clock, command, and data lines
    /// D0 through D3. Keeping them explicit prevents accidental mismatch with
    /// the board wiring.
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

    /// Read one 512-byte sector from the card.
    pub async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        // Embassy's SDMMC API uses a fixed-size `DataBlock`; copy into the
        // caller's slice only after the hardware read succeeds.
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

    /// Write one 512-byte sector to the card.
    pub async fn write_block(&mut self, lba: u32, buf: &[u8]) -> Result<(), ()> {
        // Copy first so the DMA-visible block has exactly the size expected by
        // the SDMMC driver.
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
