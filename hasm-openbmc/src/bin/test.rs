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
use panic_probe as _;

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

macro_rules! make_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.init($val)
    }};
}

#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>) {
    usb.run().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // 168MHz SYSCLK, 48MHz USB clock from HSE=25MHz
    let mut config = embassy_stm32::Config::default();
    config.rcc.hse = Some(Hse {
        freq: Hertz(25_000_000),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV25,
        mul: PllMul::MUL336,
        divp: Some(PllPDiv::DIV2),
        divq: Some(PllQDiv::DIV7),
        divr: None,
    });
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;

    let p = embassy_stm32::init(config);
    info!("STM32F407 initialized");

    // USB OTG FS on PA12(DP), PA11(DM)
    let ep_out_buffer = make_static!([u8; 256], [0; 256]);
    let mut usb_config = UsbConfig::default();
    usb_config.vbus_detection = false;
    let driver = Driver::new_fs(p.USB_OTG_FS, Irqs, p.PA12, p.PA11, ep_out_buffer, usb_config);

    let config_descriptor = make_static!([u8; 256], [0; 256]);
    let bos_descriptor = make_static!([u8; 256], [0; 256]);
    let control_buf = make_static!([u8; 64], [0; 64]);

    let mut usb_config = embassy_usb::Config::new(0xc0de, 0xcafe);
    usb_config.manufacturer = Some("MyBMC");
    usb_config.product = Some("STM32F407 USB MSC");
    usb_config.serial_number = Some("F407-MSC-001");
    usb_config.max_power = 100;
    usb_config.max_packet_size_0 = 64;

     let mut builder = Builder::new(
        driver,
        usb_config,
        config_descriptor,
        bos_descriptor,
        &mut [],
        control_buf,
    );
    // ==========================================
    // 关键魔法时刻：手动构建 MSC 专属接口与端点
    // ==========================================
    // 0x08 = Mass Storage Class (MSC) (U盘分类)
    // 0x06 = SCSI 透传协议 (Subclass)
    // 0x50 = Bulk-Only Transport (BOT) 协议
    let mut function = builder.function(0x08, 0x06, 0x50);
    
    // 初始化该功能的接口
    let mut interface = function.interface();
    let mut alt_setting = interface.alt_setting(0x08, 0x06, 0x50, None);
    
    // 申请两个 Bulk 端点，这是 U 盘读写数据的专用通道
    // 参数1: None (由 Embassy 自动分配端点地址: 0x01, 0x81 等)
    // 参数2: 64 (全速 USB Bulk 传输的最大包长度)
    let _ep_out = alt_setting.endpoint_bulk_out(None, 64);
    let _ep_in = alt_setting.endpoint_bulk_in(None, 64);
    
    // 必须在此刻 drop(function)，将控制权交还给 builder
    drop(function);
    // ==========================================
    let usb = builder.build();
    unwrap!(spawner.spawn(usb_task(usb)));

    info!("✓ USB Device is running");
    info!("✓ VID=0xc0de, PID=0xcafe, Mfr=MyBMC");
    info!("✓ Should enumerate on PC now");
    info!("⚠ Note: MSC class not yet implemented - device will appear as 'Mass Storage Device' but may not mount");
    
    loop {
        Timer::after_secs(10).await;
    }
}