use defmt::info;
use embassy_stm32::{Peri, gpio::{Level, Output, Speed}, peripherals::PF6};

use crate::services::power_control::POWER_SIGNAL;


#[embassy_executor::task]
pub async fn led_task(mut led: Output<'static>) -> ! {
    loop {
        let power_on = POWER_SIGNAL.wait().await;
        if power_on {
            led.set_low(); 
            // info!("[BMC] Power ON -> LED Lit");
        } else {
            led.set_high();
            // info!("[BMC] Power OFF -> LED Off");
        }
    }
}

pub fn led_init(pin: Peri<'static, PF6>, initial_output: Level, speed: Speed) -> Output<'static> {
    Output::new(pin, initial_output, speed)
}