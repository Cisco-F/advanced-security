use defmt::warn;
use embassy_time::Timer;

use crate::block::BlockDevice;
use crate::block::cached_data::CachedData;
use crate::block::example_fs::ExampleBlockDevice;
use crate::block::remote::RemoteBlockDevice;
use crate::block::tf::TfBlockDevice;
use crate::drivers::usb_msc::device::MSCDev;
use crate::drivers::usb_msc::device::ScsiDataSink;
use crate::drivers::usb_msc::scsi::{self, ScsiResponse};
use crate::drivers::usb_msc::transport::Cbw;
use crate::drivers::usb_msc::transport::Csw;
use embassy_stm32::usb::Driver;
use embassy_stm32::peripherals::USB_OTG_FS;
use embassy_usb::UsbDevice;

#[embassy_executor::task]
pub async fn usb_device_task(
    mut usb: UsbDevice<'static, Driver<'static, USB_OTG_FS>>,
) -> ! {
    usb.run().await
}

#[embassy_executor::task]
pub async fn tf_usb_task(bdev: CachedData<TfBlockDevice>, sink: MSCDev<Driver<'static, USB_OTG_FS>>) -> ! {
    run_usb(bdev, sink).await
}

#[embassy_executor::task]
pub async fn remote_usb_task(bdev: CachedData<RemoteBlockDevice>, sink: MSCDev<Driver<'static, USB_OTG_FS>>) -> ! {
    run_usb(bdev, sink).await
}

#[embassy_executor::task]
pub async fn example_usb_task(bdev: CachedData<ExampleBlockDevice>, sink: MSCDev<Driver<'static, USB_OTG_FS>>) -> ! {
    run_usb(bdev, sink).await
}

async fn run_usb<D: BlockDevice>(mut cached_bdev: CachedData<D>, mut sink: MSCDev<Driver<'static, USB_OTG_FS>>) -> ! {
    let mut cbw_buf = [0u8; 31];

    loop {
        let _ = match sink.read(&mut cbw_buf).await {
            Ok(_) => (),
            Err(e) => {
                warn!("MSC OUT read error: {:?}", e);
                Timer::after_millis(1000).await;
                continue;
            }
        };

        let cbw = Cbw::from_bytes(&cbw_buf);
        let response: ScsiResponse = scsi::handle_scsi_cmd(&mut cached_bdev, &mut sink, cbw).await;

        let csw = Csw::new(cbw.tag, response.residue, response.status as u8);
        let csw_buf = csw.to_bytes();
        if let Err(e) = sink.write(&csw_buf).await {
            warn!("MSC CSW write error: {:?}", e.usb_error);
        }
    }
}