#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    peripherals::{self, ETH},
    usb::Driver,
};
use hasm_openbmc::{block::{cached_data::CachedData, remote::RemoteBlockDevice}, consts::IP, drivers::{ethernet::ethernet_device, usb_msc::{device::ScsiDataSink, scsi::{CSW_SIGNATURE, handle_scsi_cmd}, transport::Cbw}}, hal::init::sys_init, net::init_eth_stack};
use {defmt_rtt as _, panic_probe as _};
use embassy_time::Timer;
use embassy_stm32::
    eth::{Ethernet, GenericPhy}
;
use core::net::Ipv4Addr;
use panic_probe as _;


#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>) {
    usb.run().await;
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Ethernet<'static, ETH, GenericPhy>>) -> ! {
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = sys_init();

    // 网络初始化
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
    unwrap!(spawner.spawn(net_task(runner)));

    // 创建msc设备
    let mut msc_dev = hasm_openbmc::drivers::usb_msc::device::MSCDev::init();
    msc_dev.new(p.USB_OTG_FS, p.PA12, p.PA11);
    unwrap!(spawner.spawn(usb_task(msc_dev.usb_device.take().unwrap())));

    let ip = Ipv4Addr::new(192, 168, 1, 77);
    let port = 8000;
    let bdev = RemoteBlockDevice::new(stack, ip, port);
    let mut cached_bdev = CachedData::new(bdev);

    let mut cbw_buf = [0u8; 31];

    loop {
        let _ = match msc_dev.read(&mut cbw_buf).await {
            Ok(_) => (),
            Err(e) => {
                warn!("MSC OUT read error: {:?}", e);
                Timer::after_millis(500).await;
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