//! Ethernet peripheral construction for the STM32F407 RMII interface.
//!
//! The board wiring follows the common LAN8720-style RMII pinout:
//! REFCLK, MDIO, MDC, CRS_DV, RXD0/RXD1, TXD0/TXD1, and TX_EN.
//! Keeping the pin names in the function signature makes the hardware contract
//! visible at the call site and lets Embassy prevent accidental reuse.
//!
//! The packet queue depth is intentionally small. This firmware handles low
//! concurrency management traffic and boot-image reads, so a larger queue would
//! mostly consume SRAM without improving throughput.

use embassy_stm32::{
  Peri, bind_interrupts,
  eth::{self, Ethernet, GenericPhy, PacketQueue},
  peripherals::{ETH, PA1, PA2, PC1, PA7, PC4, PC5, PG13, PG14, PG11},
};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};
use crate::consts::*;

static PACKETS: StaticCell<PacketQueue<4, 4>> = StaticCell::new();

bind_interrupts!(struct EthernetIrqs {
  ETH => eth::InterruptHandler;
});

/// Build the Embassy Ethernet device from the board's RMII pins.
pub fn ethernet_device(
  eth: Peri<'static, ETH>,
  ref_clk: Peri<'static, PA1>,
  mdio: Peri<'static, PA2>,
  mdc: Peri<'static, PC1>,
  crs: Peri<'static, PA7>,
  rx_d0: Peri<'static, PC4>,
  rx_d1: Peri<'static, PC5>,
  tx_d0: Peri<'static, PG13>,
  tx_d1: Peri<'static, PG14>,
  tx_en: Peri<'static, PG11>,
) -> Ethernet<'static, ETH, GenericPhy> {
  // The PHY address is zero for the default board design. If a different PHY
  // strap configuration is used, update `GenericPhy::new` here.
  Ethernet::new(
    PACKETS.init(PacketQueue::new()),
    eth,
    EthernetIrqs,
    ref_clk,  // REFCLK
    mdio,  // MDIO
    mdc,  // MDC
    crs,  // CRS_DV
    rx_d0,  // RXD0
    rx_d1,  // RXD1
    tx_d0, // TXD0
    tx_d1, // TXD1
    tx_en, // TX_EN
    GenericPhy::new(0),
    MAC,
  )
}
