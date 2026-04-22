use embassy_stm32::{Peri, gpio::{Level, Output, Speed}, peripherals::PF6};
use embassy_time::Timer;

use crate::services::power_control::is_power_on;


#[embassy_executor::task]
pub async fn led_task(mut led: Output<'static>) -> ! {
    let mut last_state = is_power_on();
    if last_state {
        led.set_low();
    } else {
        led.set_high();
    }

    loop {
        let power_on = is_power_on();
        if power_on != last_state {
            if power_on {
                led.set_low();
            } else {
                led.set_high();
            }
            last_state = power_on;
        }
        Timer::after_millis(200).await;
    }
}

pub fn led_init(pin: Peri<'static, PF6>, initial_output: Level, speed: Speed) -> Output<'static> {
    Output::new(pin, initial_output, speed)
}