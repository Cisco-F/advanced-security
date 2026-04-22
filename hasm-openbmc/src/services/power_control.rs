use defmt::info;
use embassy_stm32::{Peri, gpio::{Level, Output, Speed}, peripherals::{PB3, PB4}};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embassy_time::Timer;


pub static POWER_SIGNAL: Signal<ThreadModeRawMutex, bool> = Signal::new();

#[embassy_executor::task]
pub async fn power_task(mut power_control: PowerControl) -> ! {
    loop {
        let state = POWER_SIGNAL.wait().await;
        if state {
            power_control.power_on().await;
        } else {
            power_control.power_off().await;
        }
    }
}

pub struct PowerControl {
    pub state: bool,
    power_off_pin: Output<'static>,
    power_on_pin: Output<'static>
}

impl PowerControl {
    pub fn new(power_off_pin: Peri<'static, PB3>, power_on_pin: Peri<'static, PB4>) -> Self {
        let power_off_pin = Output::new(power_off_pin, Level::High, Speed::Low);
        let power_on_pin = Output::new(power_on_pin, Level::High, Speed::Low);
        Self {
            state: true,
            power_off_pin,
            power_on_pin
        }
    }

    pub async fn power_on(&mut self) {
        if self.state {
            info!("Power is already ON, ignoring power on request");
            return;
        }

        info!("Power ON: asserting power pin");
        POWER_SIGNAL.signal(true);
        self.power_on_pin.set_low();
        Timer::after_secs(3).await;
        self.power_on_pin.set_high();
        self.state = true;
        info!("Power on complete");
        defmt::flush();
    }

    pub async fn power_off(&mut self) {
        if !self.state {
            info!("Power is already OFF, ignoring power off request");
            return;
        }

        info!("Power OFF: asserting power pin");
        POWER_SIGNAL.signal(false);
        self.power_off_pin.set_low();
        Timer::after_secs(3).await;
        self.power_off_pin.set_high();
        self.state = false;
        info!("Power off complete");
        defmt::flush();
    }
}