#![no_std]

pub mod web_server;
pub mod vnc_server;
pub mod power_control;
pub mod scsi;
pub mod utils;

use embassy_stm32::Config;
use embassy_stm32::rcc::*;
use embassy_stm32::time::Hertz;

pub fn clk_init() -> Config {
    let mut config = Config::default();
    
    config.rcc.hse = Some(Hse {
        freq: Hertz(25_000_000), 
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV25,
        mul: PllMul::MUL336,
        divp: Some(PllPDiv::DIV2), // 168MHz
        divq: Some(PllQDiv::DIV7),
        divr: None,
    });
    config.rcc.sys = Sysclk::PLL1_P; 
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;

    config
}