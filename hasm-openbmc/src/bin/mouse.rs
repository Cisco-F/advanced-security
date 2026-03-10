#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _; // 基于 probe-rs 的 RTT 打印
use panic_probe as _;

use embassy_executor::Spawner;
use embassy_stm32::rcc::{
    AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllSource, Sysclk,
};
use embassy_stm32::time::Hertz;
use embassy_stm32::usb::{Config as UsbConfig, Driver};
use embassy_stm32::{bind_interrupts, peripherals, usb};
use embassy_time::Timer;
use embassy_usb::class::hid::{Config, HidWriter, State};
use embassy_usb::Builder;
use usbd_hid::descriptor::{MouseReport, SerializedDescriptor};

// 绑定 USB OTG FS 的中断
bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

// 用于跨任务的 USB 运行宏
#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>) {
    // 启动 USB 栈后台循环
    usb.run().await;
}

// 分配静态内存的小宏
macro_rules! make_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.init($val)
    }};
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // 1. 系统与时钟配置 (野火霸天虎V2为STM32F407ZGT6, 25MHz外部晶振)
    let mut config = embassy_stm32::Config::default();
    config.rcc.hse = Some(Hse {
        freq: Hertz(25_000_000),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV25, // 25MHz / 25 = 1MHz
        mul: PllMul::MUL336,      // 1MHz * 336 = 336MHz
        divp: Some(PllPDiv::DIV2),// 336MHz / 2 = 168MHz (系统最高主频)
        divq: Some(PllQDiv::DIV7),// 336MHz / 7 = 48MHz (供USB使用, 必须是48)
        divr: None,
    });
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;

    let p = embassy_stm32::init(config);
    info!("系统时钟初始化完成, 准备配置USB...");

    // 2. 配置 USB 驱动 (STM32F407 OTG FS: DP=PA12, DM=PA11)
    let ep_out_buffer = make_static!([u8; 256], [0; 256]);
    let mut usb_config = UsbConfig::default();
    // 忽略 VBUS 检测（开发板有时没把 VBUS 引脚接上，设为 false 可以避免枚举失败）
    usb_config.vbus_detection = false; 
    let driver = Driver::new_fs(p.USB_OTG_FS, Irqs, p.PA12, p.PA11, ep_out_buffer, usb_config);

    // 3. 配置 Embassy USB Builder 所需的静区缓冲区
    let config_descriptor = make_static!([u8; 256], [0; 256]);
    let bos_descriptor = make_static!([u8; 256], [0; 256]);
    let control_buf = make_static!([u8; 64], [0; 64]);

    let mut usb_config = embassy_usb::Config::new(0xc0de, 0xcafe);
    usb_config.manufacturer = Some("Wildfire");
    usb_config.product = Some("Embassy HID Mouse");
    usb_config.serial_number = Some("123456");
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
    

    // 4. 配置 HID 类描述符与状态
    let hid_config = Config {
        report_descriptor: MouseReport::desc(),
        request_handler: None,
        poll_ms: 10,       // 鼠标上报间隔 (毫秒)
        max_packet_size: 8,
    };
    let hid_state = make_static!(State, State::new());
    let mut hid_writer = HidWriter::<_, 8>::new(&mut builder, hid_state, hid_config);

    // 5. 构建 USB 并放入后台任务执行
    let usb = builder.build();
    unwrap!(spawner.spawn(usb_task(usb)));

    info!("USB 设备已启动! 你可以将其插入电脑。");

    // 6. 鼠标事件主循环
    loop {
        // 创建一个鼠标测试事件（例如：鼠标一直往右下角缓慢移动）
        let report = MouseReport {
            buttons: 0, // 0表示无按键按下, 1:左键, 2:右键
            x: 5,       // 水平偏移量 (-127 到 127)
            y: 5,       // 垂直偏移量 (-127 到 127)
            wheel: 0,   // 滚轮
            pan: 0,     // 中键滚动
        };

        // 尝试发送到主机
        match hid_writer.write_serialize(&report).await {
            Ok(_) => {
                // 如果发送成功（主机已识别并挂载），捕捉该事件打印到 probe-rs 控制台
                info!(
                    "【捕捉到鼠标事件】发送位移 -> X: {=i8}, Y: {=i8}",
                    report.x, report.y
                );
            }
            Err(_) => {
                // 电脑没插紧、还未拔插、或者进入了挂起状态
                warn!("等待 USB 主机连接...");
            }
        }

        // 停顿 1 秒再执行下一步
        Timer::after_millis(1000).await;
    }
}