#![no_std]
#![no_main]

//! Firmware entry point for the STM32F407 BMC board.
//!
//! Startup order matters in this file:
//! 1. Clock and peripheral ownership are established first.
//! 2. Ethernet is configured and the Embassy network runner is spawned.
//! 3. User-facing services are spawned only after the interface has an address.
//! 4. USB MSC is built last because it depends on an initialized block backend.
//!
//! The controlled Raspberry Pi is expected to see this board as three things:
//! a UART console bridge, a power-control fixture, and a USB mass-storage boot
//! device. The host PC simultaneously sees a small Redfish-like HTTP API and a
//! byte-range image server client.
//!
//! The image backend selected here is remote HTTP storage. Examples in the
//! repository show the same USB task running against a TF card or a synthetic
//! FAT image, which is why the USB service is generic over `BlockDevice`.
//!
//! Because this is embedded firmware, initialization code favors explicit pin
//! wiring over compact abstractions. The pin list doubles as the board bring-up
//! checklist and should stay close to the actual schematic.

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Speed};
use {defmt_rtt as _, panic_probe as _};
use hasm_openbmc::{
    block::{cached_data::CachedData, remote::RemoteBlockDevice},
    config::{get_board_ip, get_host_ip},
    consts::{IMG_SERVER_PORT, UART_BAUDRATE},
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
        virtual_usb::{remote_usb_task, usb_device_task},
        web_server::http_task,
    },
};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = sys_init();

    // RMII Ethernet consumes a fixed group of alternate-function pins on the
    // STM32F407. Passing them here makes accidental pin reuse a compile-time
    // error because Embassy's peripheral tokens can be moved only once.
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
    // The network runner owns packet RX/TX progress. All TCP services below
    // share the lightweight `Stack` handle and would stall without this task.
    unwrap!(spawner.spawn(net_task(runner)));

    // Initialize UART console control.
    let ip = get_board_ip();
    let uart = uart_init(p.USART1, p.PA10, p.PA9, UART_BAUDRATE);
    info!("UART ready: Raspberry Pi TXD -> STM32 PA10 (USART1_RX)");
    info!("UART ready: optional Raspberry Pi RXD -> STM32 PA9 (USART1_TX)");
    info!("UART ready: open tcp://{}:{} before powering on the Raspberry Pi", ip, UART_BAUDRATE);

    unwrap!(spawner.spawn(console_task(uart, stack)));

    // Initialize the power-management module.
    // PB3/PB4 emulate the two board-level power buttons. The pins idle high and
    // are pulled low for a short pulse when Redfish requests a state change.
    let power_control = PowerControl::new(p.PB3, p.PB4);
    let led = led_init(p.PF6, Level::Low, Speed::Low);

    let _ = unwrap!(spawner.spawn(http_task(stack)));
    let _ = unwrap!(spawner.spawn(power_task(power_control)));
    let _ = unwrap!(spawner.spawn(led_task(led)));

    // Initialize the emulated USB MSC device.
    // USB OTG FS is exposed as a read-only mass-storage device. The USB device
    // runner and the SCSI command loop are separate tasks: the former handles
    // enumeration/control traffic, while the latter moves bulk MSC payloads.
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
    
    // The remote image server supports HTTP byte ranges. Wrapping it with a
    // small sector cache reduces TCP reconnects during boot, when the Pi often
    // rereads adjacent LBAs while probing partitions and firmware files.
    let ip = get_host_ip();
    let port = IMG_SERVER_PORT;
    let bdev = RemoteBlockDevice::new(stack, ip, port);
    let cached_bdev = CachedData::new(bdev);
    
    unwrap!(spawner.spawn(remote_usb_task(cached_bdev, msc_dev)));

    info!("✓ USB MSC device ready!");

}
