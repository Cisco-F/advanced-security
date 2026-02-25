#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_stm32::usart::{Config, Uart};
use embassy_time::{Duration, Timer};
use defmt::*;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());

    let mut cfg = Config::default();
    cfg.baudrate = 115200;

    let mut uart = Uart::new_blocking(
        p.USART3,
        p.PB11,
        p.PB10,
        cfg,
    ).unwrap();

    info!("ESP8266 start");

    uart.blocking_write(b"AT\r\n").unwrap();
    Timer::after(Duration::from_secs(1)).await;

    uart.blocking_write(b"AT+CWMODE=1\r\n").unwrap();
    Timer::after(Duration::from_secs(1)).await;

    // 修改成你的 WiFi
    uart.blocking_write(
        b"AT+CWJAP=\"CMCC_HuKs\",\"Fwb2004326@\"\r\n"
    ).unwrap();

    Timer::after(Duration::from_secs(10)).await;

    uart.blocking_write(
        b"AT+CIPSTA=\"192.168.1.177\",\"192.168.1.1\",\"255.255.255.0\"\r\n"
    ).unwrap();

    loop {
        Timer::after(Duration::from_secs(1)).await;
    }
}