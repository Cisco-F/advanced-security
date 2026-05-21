#![no_std]
#![no_main]

//! USB MSC example backed by a physical TF card.
//!
//! Exposes the SDIO TF-card backend as USB MSC, validating card initialization,
//! DMA-backed reads, and SCSI handling without the remote image server.

use defmt::*;
use embassy_executor::Spawner;
use {defmt_rtt as _, panic_probe as _};
use hasm_openbmc::{
    block::{cached_data::CachedData, tf::TfBlockDevice},
    drivers::usb_msc::device::MSCDev,
    hal::init::sys_init,
    services::virtual_usb::{tf_usb_task, usb_device_task},
};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = sys_init();

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
