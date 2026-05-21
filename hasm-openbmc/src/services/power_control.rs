//! Raspberry Pi power-control service.
//!
//! The board exposes two GPIO-controlled lines that behave like physical power
//! buttons on the controlled Raspberry Pi carrier. A low pulse asserts the
//! corresponding action; the pins idle high.
//!
//! `POWER_STATE` is the firmware's current desired/observed state for user-facing
//! services. `POWER_SIGNAL` lets HTTP handlers request a state transition without
//! directly owning the GPIO pins.
//!
//! Atomic ordering is relaxed because this value is not protecting memory; it is
//! a small status flag shared between cooperative Embassy tasks.

use defmt::info;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_stm32::{Peri, gpio::{Level, Output, Speed}, peripherals::{PB3, PB4}};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embassy_time::Timer;

pub static POWER_SIGNAL: Signal<ThreadModeRawMutex, bool> = Signal::new();
pub static POWER_STATE: AtomicBool = AtomicBool::new(true);

/// Request a power-state change and notify the GPIO task.
pub fn set_power_state(state: bool) {
    POWER_STATE.store(state, Ordering::Relaxed);
    POWER_SIGNAL.signal(state);
}

/// Read the latest firmware power-state flag.
pub fn is_power_on() -> bool {
    POWER_STATE.load(Ordering::Relaxed)
}

#[embassy_executor::task]
pub async fn power_task(mut power_control: PowerControl) -> ! {
    loop {
        // Serialize all power pulses through one task so PB3/PB4 are never
        // driven by concurrent request handlers.
        let state = POWER_SIGNAL.wait().await;
        POWER_STATE.store(state, Ordering::Relaxed);
        if state {
            power_control.power_on().await;
        } else {
            power_control.power_off().await;
        }
    }
}

/// GPIO owner for the two power-control lines.
pub struct PowerControl {
    /// Active-low line used to request a force-off pulse.
    power_off_pin: Output<'static>,
    /// Active-low line used to request a power-on pulse.
    power_on_pin: Output<'static>
}

impl PowerControl {
    /// Configure both control pins in their inactive high state.
    pub fn new(power_off_pin: Peri<'static, PB3>, power_on_pin: Peri<'static, PB4>) -> Self {
        let power_off_pin = Output::new(power_off_pin, Level::High, Speed::Low);
        let power_on_pin = Output::new(power_on_pin, Level::High, Speed::Low);
        Self {
            power_off_pin,
            power_on_pin
        }
    }

    /// Pulse the power-on line if the board is currently considered off.
    pub async fn power_on(&mut self) {
        if is_power_on() {
            info!("Power is already ON, ignoring power on request");
            return;
        }

        info!("Power ON: asserting power pin");
        self.power_on_pin.set_low();
        Timer::after_secs(3).await;
        self.power_on_pin.set_high();
        info!("Power on complete");
        defmt::flush();
    }

    /// Pulse the power-off line if the board is currently considered on.
    pub async fn power_off(&mut self) {
        if !is_power_on() {
            info!("Power is already OFF, ignoring power off request");
            return;
        }

        info!("Power OFF: asserting power pin");
        self.power_off_pin.set_low();
        Timer::after_secs(3).await;
        self.power_off_pin.set_high();
        info!("Power off complete");
        defmt::flush();
    }
}
