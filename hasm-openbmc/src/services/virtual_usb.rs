//! USB mass-storage service tasks.
//!
//! Embassy requires the USB device runner to run continuously so enumeration,
//! control transfers, suspend/resume, and endpoint wakeups are handled. The SCSI
//! command loop is kept in a separate task that owns the bulk endpoints through
//! `MSCDev`.
//!
//! Three wrapper tasks exist because Embassy task functions need concrete types.
//! The shared `run_usb` body is generic over the block backend so the same SCSI
//! logic can serve a TF card, the remote HTTP image, or the example filesystem.

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
    // Never returns: USB bus state is driven from inside Embassy's device future.
    usb.run().await
}

/// Run USB MSC with a physical TF-card backend.
#[embassy_executor::task]
pub async fn tf_usb_task(bdev: CachedData<TfBlockDevice>, sink: MSCDev<Driver<'static, USB_OTG_FS>>) -> ! {
    run_usb(bdev, sink).await
}

/// Run USB MSC with a host-side HTTP range image backend.
#[embassy_executor::task]
pub async fn remote_usb_task(bdev: CachedData<RemoteBlockDevice>, sink: MSCDev<Driver<'static, USB_OTG_FS>>) -> ! {
    run_usb(bdev, sink).await
}

/// Run USB MSC with the in-memory example FAT image backend.
#[embassy_executor::task]
pub async fn example_usb_task(bdev: CachedData<ExampleBlockDevice>, sink: MSCDev<Driver<'static, USB_OTG_FS>>) -> ! {
    run_usb(bdev, sink).await
}

/// Main MSC command loop.
///
/// Each iteration reads one CBW, dispatches the contained SCSI command, and
/// sends one CSW. Bulk-Only Transport depends on that strict command/data/status
/// ordering, so errors are logged and the loop waits for the next host command.
async fn run_usb<D: BlockDevice>(mut cached_bdev: CachedData<D>, mut sink: MSCDev<Driver<'static, USB_OTG_FS>>) -> ! {
    let mut cbw_buf = [0u8; 31];

    loop {
        let _ = match sink.read(&mut cbw_buf).await {
            Ok(_) => (),
            Err(e) => {
                // Transport errors usually mean the host reset the device or the
                // cable was unplugged. Delay before retrying so logs remain
                // readable during repeated failures.
                warn!("MSC OUT read error: {:?}", e);
                Timer::after_millis(1000).await;
                continue;
            }
        };

        let cbw = Cbw::from_bytes(&cbw_buf);
        // `handle_scsi_cmd` performs any data phase before returning status.
        let response: ScsiResponse = scsi::handle_scsi_cmd(&mut cached_bdev, &mut sink, cbw).await;

        // Echo the CBW tag so the host can correlate status with the command it
        // just issued.
        let csw = Csw::new(cbw.tag, response.residue, response.status as u8);
        let csw_buf = csw.to_bytes();
        if let Err(e) = sink.write(&csw_buf).await {
            warn!("MSC CSW write error: {:?}", e.usb_error);
        }
    }
}
