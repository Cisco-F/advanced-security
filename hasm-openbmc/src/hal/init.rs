//! MCU clock tree initialization.
//!
//! The firmware targets an STM32F407ZG board with a 25 MHz external oscillator.
//! PLL settings below produce a 168 MHz system clock, which is the common maximum
//! operating point for the F407 and gives enough headroom for Ethernet, USB FS,
//! SDIO, and UART services running together.
//!
//! APB1 is divided by 4 and APB2 by 2 to keep peripheral buses within their data
//! sheet limits while the core runs at full speed.

use embassy_stm32::Config;
use embassy_stm32::Peripherals;
use embassy_stm32::rcc::*;
use embassy_stm32::time::Hertz;

/// Initialize STM32 peripherals and return Embassy's ownership tokens.
pub fn sys_init() -> Peripherals {
    let mut config = Config::default();
    
    // Use the board crystal rather than HSI so Ethernet RMII and USB timing are
    // derived from a stable external source.
    config.rcc.hse = Some(Hse {
        freq: Hertz(25_000_000), 
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;
    // 25 MHz / 25 * 336 / 2 = 168 MHz core clock.
    // divq=7 gives a 48 MHz domain suitable for USB FS.
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

    embassy_stm32::init(config)
}
