use defmt::*;
use embassy_stm32::gpio::Output;
use crate::web_server::POWER_SIGNAL;

#[embassy_executor::task]
/// we use led to represente power state for now
pub async fn led_task(mut led: Output<'static>) -> ! {
    loop {
        let power_on = POWER_SIGNAL.wait().await;
        if power_on {
            led.set_low(); 
            info!("[BMC] Power ON -> LED Lit");
        } else {
            led.set_high();
            info!("[BMC] Power OFF -> LED Off");
        }
    }
}