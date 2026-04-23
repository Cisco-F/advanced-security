#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use {defmt_rtt as _, panic_probe as _};
use hasm_openbmc::{
    block::{cached_data::CachedData, example_fs::ExampleBlockDevice},
    drivers::usb_msc::device::MSCDev,
    hal::init::sys_init,
    services::virtual_usb::{example_usb_task, usb_device_task}
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

    let bdev = ExampleBlockDevice;
    let cached_bdev = CachedData::new(bdev);
    
    unwrap!(spawner.spawn(example_usb_task(cached_bdev, msc_dev)));

    info!("✓ USB MSC device ready!");

}
