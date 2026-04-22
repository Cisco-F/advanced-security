#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::rcc::{
    AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllSource,
    Sysclk,
};
use embassy_stm32::time::Hertz;
use embassy_stm32::usb::{Config as UsbConfig, Driver};
use embassy_stm32::{bind_interrupts, peripherals, usb};
use embassy_time::Timer;
use embassy_usb::Builder;
use embassy_usb::driver::{EndpointIn, EndpointOut};
use hasm_openbmc::drivers::usb_msc::device::ScsiDataSink;
use hasm_openbmc::drivers::usb_msc::scsi::{CSW_SIGNATURE, handle_scsi_cmd};
use hasm_openbmc::drivers::usb_msc::transport::Cbw;
use hasm_openbmc::block::example_fs::ExampleBlockDevice;
use panic_probe as _;


#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>) {
    usb.run().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = hasm_openbmc::hal::init::sys_init();
    info!("✓ Clock init");

    let mut msc_dev = hasm_openbmc::drivers::usb_msc::device::MSCDev::init();
    msc_dev.new(p.USB_OTG_FS, p.PA12, p.PA11);
    unwrap!(spawner.spawn(usb_task(msc_dev.usb_device.take().unwrap())));

    let mut bdev = ExampleBlockDevice; 

    info!("✓ USB MSC device ready!");

    let mut cbw_buf = [0u8; 31];
    
    // 主缓冲区，4096 字节已经绰绰有余应付 Windows 发起的元数据查阅了！
    let mut data_buf = [0u8; 4096]; 

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
        let mut response = handle_scsi_cmd(&mut bdev, &mut msc_dev, cbw).await;

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