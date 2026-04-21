#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::peripherals;
use embassy_stm32::usb::Driver;
use embassy_time::Timer;
use hasm_openbmc::block::cached_data::CachedData;
use hasm_openbmc::drivers::usb_msc::device::ScsiDataSink;
use hasm_openbmc::drivers::usb_msc::scsi::{CSW_SIGNATURE, handle_scsi_cmd};
use hasm_openbmc::drivers::usb_msc::transport::Cbw;
use hasm_openbmc::block::tf::TfBlockDevice;
use panic_probe as _;

#[embassy_executor::task]
async fn usb_task(
    mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>,
) {
    usb.run().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = hasm_openbmc::hal::init::sys_init();
    info!("✓ Clock init");

    let mut msc_dev = hasm_openbmc::drivers::usb_msc::device::MSCDev::init();
    msc_dev.new(p.USB_OTG_FS, p.PA12, p.PA11);
    unwrap!(spawner.spawn(usb_task(msc_dev.usb_device.take().unwrap())));

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
    let mut cached_bdev = CachedData::new(bdev);

    info!("✓ USB MSC device ready!");

    let mut cbw_buf = [0u8; 31];

    loop {
        let n = match msc_dev.read(&mut cbw_buf).await {
            Ok(n) => n,
            Err(e) => {
                warn!("MSC OUT read error: {:?}", e);
                Timer::after_millis(10).await;
                continue;
            }
        };

        let cbw = Cbw::from_bytes(&cbw_buf);
        let response = handle_scsi_cmd(&mut cached_bdev, &mut msc_dev, cbw).await;

        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        csw[4..8].copy_from_slice(&cbw.tag.to_le_bytes());
        csw[8..12].copy_from_slice(&response.residue.to_le_bytes());
        csw[12] = response.status as u8;

        if let Err(e) = msc_dev.write(&csw).await {
            warn!("MSC CSW write error: {:?}", e.usb_error);
        }
    }
}
