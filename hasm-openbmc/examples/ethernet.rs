#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_net::{StackResources, Ipv4Address, StaticConfigV4, Ipv4Cidr};
use embassy_stm32::{
    bind_interrupts,
    eth::{self, Ethernet, GenericPhy, PacketQueue},
    Config as StmConfig,
};
use embassy_time::{Timer, Duration};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};
use embassy_stm32::rcc::*;
use embassy_stm32::time::Hertz;
use heapless::Vec;

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
});

static PACKETS: StaticCell<PacketQueue<4, 4>> = StaticCell::new();
static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

type Device = Ethernet<'static, embassy_stm32::peripherals::ETH, GenericPhy>;

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Device>) -> ! {
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = StmConfig::default();
    {
        config.rcc.hse = Some(Hse {
            freq: Hertz(25_000_000), 
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV25,
            mul: PllMul::MUL336,
            divp: Some(PllPDiv::DIV2), // 168MHz
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        // 【修正3】STM32F4 的 PLL 系统时钟枚举变更为 Pll1_P (注意下划线)
        config.rcc.sys = Sysclk::PLL1_P; 
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }
    let p = embassy_stm32::init(config);
    info!("LAN8720 test start");

    let mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];

    let device = Ethernet::new(
        PACKETS.init(PacketQueue::new()),
        p.ETH,
        Irqs,

        // RMII
        p.PA1,  // REFCLK
        p.PA2,  // MDIO
        p.PC1,  // MDC
        p.PA7,  // CRS_DV
        p.PC4,  // RXD0
        p.PC5,  // RXD1

        p.PG13, // TX_EN
        p.PG14, // TXD0
        p.PG11, // TXD1

        GenericPhy::new(0),
        mac,
    );

    // 使用静态ip
    // 设置ip
    let address = Ipv4Address::new(192, 168, 1, 177);
    let cidr = Ipv4Cidr::new(address, 24);

    // 设置网关
    let gateway = Ipv4Address::new(192, 168, 1, 1);

    // 设置dns服务器（这里留空）
    let dns_servers: Vec<Ipv4Address, 3> = Vec::new();

    let static_config = StaticConfigV4 {
        address: cidr,
        gateway: Some(gateway),
        dns_servers: dns_servers,
    };
    let config = embassy_net::Config::ipv4_static(static_config);

    let (stack, runner) = embassy_net::new(
        device,
        config,
        RESOURCES.init(StackResources::new()),
        0x12345678,
    );

    spawner.spawn(net_task(runner)).unwrap();

    loop {
        info!("link: {}", stack.is_link_up());
        Timer::after(Duration::from_secs(1)).await;
    }
}