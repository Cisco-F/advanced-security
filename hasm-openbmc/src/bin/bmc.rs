#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_net::{Ipv4Cidr, StackResources, Ipv4Address, StaticConfigV4};
use embassy_stm32::{
    Config, bind_interrupts, 
    eth::{self, Ethernet, GenericPhy, PacketQueue}, 
    gpio::{Level, Output, Speed}, 
    peripherals::ETH, time::Hertz
};
use embassy_stm32::rcc::*;
use heapless::Vec;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

use hasm_openbmc::vnc_server::*;
use hasm_openbmc::web_server::*;
use hasm_openbmc::power_control::*;

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
});

static PACKETS: StaticCell<PacketQueue<4, 4>> = StaticCell::new();
static NET_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Ethernet<'static, ETH, GenericPhy>>) -> ! {
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // 时钟配置
    let mut config = Config::default();
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
        config.rcc.sys = Sysclk::PLL1_P; 
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }
    let p = embassy_stm32::init(config);

    info!("BMC Init...");

    // LED
    let led = Output::new(p.PF7, Level::High, Speed::Low);

    // 以太网初始化
    let mac_addr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    let eth_device = Ethernet::new(
        PACKETS.init(PacketQueue::<4, 4>::new()),
        p.ETH,
        Irqs,
        p.PA1,  // REF_CLK
        p.PA2,  // MDIO
        p.PC1,  // MDC
        p.PA7,  // CRS_DV
        p.PC4,  // RXD0
        p.PC5,  // RXD1
        p.PG13, // TX_EN
        p.PG14, // TXD0
        p.PG11, // TXD1
        GenericPhy::new(0), 
        mac_addr,
    );

    // 使用静态ip
    let address = Ipv4Address::new(192, 168, 1, 177);
    let cidr = Ipv4Cidr::new(address, 24);
    let gateway = Ipv4Address::new(192, 168, 1, 1);
    let dns_servers: Vec<Ipv4Address, 3> = Vec::new();
    let ip_config = StaticConfigV4 {
        address: cidr,
        gateway: Some(gateway),
        dns_servers: dns_servers,
    };
    let net_config = embassy_net::Config::ipv4_static(ip_config);
    let (stack, net_runner) = embassy_net::new(
        eth_device,
        net_config,
        NET_RESOURCES.init(StackResources::new()),
        0x1234_5678,
    );

    spawner.spawn(net_task(net_runner)).unwrap();
    info!("network initialized using static ip: {}", address);
    stack.wait_config_up().await;
    loop {
        if stack.is_link_up() {
            info!("eth link is up!");
            break;
        } else {
            warn!("eth link is not ready, retrying...");
        }
        embassy_time::Timer::after_secs(2).await;
    }

    spawner.spawn(led_task(led)).unwrap();
    spawner.spawn(http_task(stack)).unwrap();
    spawner.spawn(vnc_task(stack)).unwrap();
}
