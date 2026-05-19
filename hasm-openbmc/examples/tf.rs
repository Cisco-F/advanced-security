#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Speed};
use {defmt_rtt as _, panic_probe as _};
use hasm_openbmc::{
    block::{cached_data::CachedData, tf::TfBlockDevice},
    config::get_board_ip,
    consts::UART_BAUDRATE,
    drivers::{
        ethernet::ethernet_device,
        led::{led_init, led_task},
        uart::uart_init,
        usb_msc::device::MSCDev,
    },
    hal::init::sys_init,
    net::{init_eth_stack, net_task},
    services::{
        console::console_task,
        power_control::{PowerControl, power_task},
        virtual_usb::{tf_usb_task, usb_device_task},
        web_server::http_task,
    },
};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = sys_init();

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
    match bdev
        .init(
            p.SDIO, p.DMA2_CH3, p.PC12, p.PD2, p.PC8, p.PC9, p.PC10, p.PC11,
        )
        .await
    {
        Ok(_) => info!("TF Card init OK"),
        Err(_) => {
            error!("TF Card init failed");
            return;
        }
    }
    let cached_bdev = CachedData::new(bdev);
    
    unwrap!(spawner.spawn(tf_usb_task(cached_bdev, msc_dev)));

    info!("✓ USB MSC device ready!");

}
