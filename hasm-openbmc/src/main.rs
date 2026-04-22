#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Speed};
use {defmt_rtt as _, panic_probe as _};

use hasm_openbmc::{block::{cached_data::CachedData, tf::TfBlockDevice}, config::get_ip, consts::UART_BAUDRATE, drivers::{ethernet::ethernet_device, led::{led_init, led_task}, uart::uart_init, usb_msc::device::MSCDev}, hal::init::sys_init, net::{init_eth_stack, net_task}, services::{console::console_task, power_control::{PowerControl, power_task}, virtual_usb::{usb_device_task, usb_task}, web_server::http_task}};


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = sys_init();

    let eth_device = ethernet_device(
        p.ETH,
        p.PA1,
        p.PA2,
        p.PC1,
        p.PA7,
        p.PC4,
        p.PC5,
        p.PG13,
        p.PG14,
        p.PG11,
    );

    let (stack, runner) = init_eth_stack(eth_device);
    stack.wait_config_up().await;
    unwrap!(spawner.spawn(net_task(runner)));

    // 串口控制初始化
    let ip = get_ip();
    let uart = uart_init(p.USART1, p.PA10, p.PA9, UART_BAUDRATE);
    info!("UART ready: Raspberry Pi TXD -> STM32 PA10 (USART1_RX)");
    info!("UART ready: optional Raspberry Pi RXD -> STM32 PA9 (USART1_TX)");
    info!("UART ready: open tcp://{}:2323 before powering on the Raspberry Pi", ip);

    unwrap!(spawner.spawn(console_task(uart, stack)));

    // 电源管理模块初始化
    let power_control = PowerControl::new(p.PB3, p.PB4);
    let led = led_init(p.PF6, Level::Low, Speed::Low);

    let _ = unwrap!(spawner.spawn(http_task(stack)));
    let _ = unwrap!(spawner.spawn(power_task(power_control)));
    let _ = unwrap!(spawner.spawn(led_task(led)));

    // usb_msc模拟设备初始化
    let mut msc_dev = MSCDev::init();
    msc_dev.new(p.USB_OTG_FS, p.PA12, p.PA11);
    let usb = match msc_dev.usb_device.take() {
        Some(usb) => usb,
        None => {
            error!("USB device build failed");
            return;
        }
    };
    unwrap!(spawner.spawn(usb_device_task(usb)));
    
    let mut bdev = TfBlockDevice::new();
    match bdev.init(
        p.SDIO, 
        p.DMA2_CH3, 
        p.PC12, 
        p.PD2, 
        p.PC8, 
        p.PC9, 
        p.PC10, 
        p.PC11,
    ).await {
        Ok(_) => info!("TF Card init OK"),
        Err(_) => {
            error!("TF Card init failed");
            return;
        }
    }
    
    let cached_bdev = CachedData::new(bdev);
    unwrap!(spawner.spawn(usb_task(cached_bdev, msc_dev)));

    info!("✓ USB MSC device ready!");

}
