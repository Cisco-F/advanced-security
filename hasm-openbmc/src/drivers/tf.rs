use core::error::Error;

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    Config, Peripherals, bind_interrupts, peripherals::{self, SDIO}, sdmmc::{self, DataBlock, Sdmmc}, time::Hertz
};
use embassy_stm32::rcc::*;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct TfIrqs {
    SDIO => sdmmc::InterruptHandler<peripherals::SDIO>;
});

pub async fn tf_init(p: Peripherals) -> Result<Sdmmc<'static, SDIO>, sdmmc::Error> {
  let mut sdmmc = Sdmmc::new_4bit(
    p.SDIO,
    TfIrqs,
    p.DMA2_CH3,
    p.PC12, // CK
    p.PD2,  // CMD
    p.PC8,  // D0
    p.PC9,  // D1
    p.PC10, // D2
    p.PC11, // D3
    Default::default(),
  );

  match &sdmmc.init_sd_card(Hertz(400_000)).await {
    Ok(_) => info!("TF Card init OK"),
    Err(e) => {
      error!("TF Card init failed: {:?}", e);
      return Err(*e);
    }
  }

  if let Ok(card) = sdmmc.card() {
    match card {
      sdmmc::SdmmcPeripheral::SdCard(c) => info!("SD Card detected, CSD version: {}", c.csd.version()),
      sdmmc::SdmmcPeripheral::Emmc(_) => info!("eMMC detected"),
    }
  }

  Ok(sdmmc)
}

pub async fn read_tf_by_sector(sector: u32) -> Result<(), sdmmc::Error> {
  Ok(())
}