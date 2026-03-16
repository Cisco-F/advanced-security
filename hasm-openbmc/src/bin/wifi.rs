#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::bind_interrupts;
use embassy_stm32::peripherals;
use embassy_stm32::usart::{Config, InterruptHandler, Uart};
use embassy_time::{Duration, Timer, with_timeout};
use {defmt_rtt as _, panic_probe as _};

// 1. 绑定中断
bind_interrupts!(struct Irqs {
    USART3 => InterruptHandler<peripherals::USART3>;
});

// 2. 使用宏来封装发送和接收逻辑，避开闭包生命周期陷阱
macro_rules! send_at {
    ($uart:expr, $cmd:expr, $wait_time:expr) => {{
        info!("--> Sending: {}", core::str::from_utf8($cmd).unwrap_or(""));
        if let Err(e) = $uart.write($cmd).await {
            error!("UART Write Error: {:?}", e);
        } else {
            let mut buf = [0u8; 512]; // 增加缓冲区，防止大回复溢出
            // 使用 read_until_idle 配合超时
            match with_timeout($wait_time, $uart.read_until_idle(&mut buf)).await {
                Ok(Ok(n)) => {
                    let response = core::str::from_utf8(&buf[..n]).unwrap_or("Invalid UTF-8");
                    info!("<-- Received ({} bytes): \n{}", n, response);
                }
                Ok(Err(e)) => error!("UART Read Error: {:?}", e),
                Err(_) => warn!("Wait Timeout! No response from ESP8266."),
            }
        }
    }};
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    info!("System initialized.");

    // 3. 硬件使能引脚 (霸天虎 V2 专用)
    // PB12: WIFI_CH_PD, PB13: WIFI_RST
    let mut _esp_enable = Output::new(p.PB12, Level::High, Speed::Low);
    let mut _esp_reset = Output::new(p.PB13, Level::High, Speed::Low);

    Timer::after(Duration::from_millis(1000)).await;
    info!("ESP8266 Powered ON.");

    // 4. 配置异步串口
    let mut cfg = Config::default();
    cfg.baudrate = 115200;

    // USART3 引脚: RX=PB11, TX=PB10
    // DMA1 选择 (F407 USART3 对应 DMA1 Stream 1/3)
    let mut uart = Uart::new(
        p.USART3,
        p.PB11, // RX
        p.PB10, // TX
        Irqs,
        p.DMA1_CH3, // TX DMA
        p.DMA1_CH1, // RX DMA
        cfg,
    ).unwrap();

    // --- 开始执行 AT 指令 ---

    // 第一次尝试发送，看是否有反应
    send_at!(uart, b"AT\r\n", Duration::from_secs(1));

    // 设置为 Station 模式
    send_at!(uart, b"AT+CWMODE=1\r\n", Duration::from_secs(1));

    // 开启回显（方便调试，能看到自己发的指令）
    send_at!(uart, b"ATE1\r\n", Duration::from_secs(1));

    // 连接 WiFi
    info!("Connecting to WiFi...");
    send_at!(uart, b"AT+CWJAP=\"IPADS-Robot\",\"ipads123\"\r\n", Duration::from_secs(15));

    // 查询 IP
    send_at!(uart, b"AT+CIPSTA?\r\n", Duration::from_secs(2));

    info!("Finished, entering loop.");
    loop {
        Timer::after(Duration::from_secs(10)).await;
        send_at!(uart, b"AT\r\n", Duration::from_secs(1));
    }
}