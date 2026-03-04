//! BMC (Baseboard Management Controller) - 带外管理第一阶段：上下电控制
//!
//! 硬件: STM32F407ZG + LAN8720A (RMII)
//!
//! RMII 标准引脚连接（STM32F407ZG AF11）：
//! ┌──────────────┬──────────────┬──────────────────────────────┐
//! │ LAN8720A     │ STM32F407ZG  │ 说明                         │
//! ├──────────────┼──────────────┼──────────────────────────────┤
//! │ XTAL1/CLKIN  │ PA1          │ RMII_REF_CLK (50MHz)         │
//! │ MDIO         │ PA2          │ MDIO                         │
//! │ CRS_DV       │ PA7          │ RMII_CRS_DV                  │
//! │ MDC          │ PC1          │ MDC                          │
//! │ RXD0         │ PC4          │ RMII_RXD0                    │
//! │ RXD1         │ PC5          │ RMII_RXD1                    │
//! │ TXEN         │ PB11         │ RMII_TX_EN (备选：PG11)      │
//! │ TXD0         │ PB12         │ RMII_TXD0  (备选：PG13)      │
//! │ TXD1         │ PB13         │ RMII_TXD1  (备选：PG14)      │
//! └──────────────┴──────────────┴──────────────────────────────┘
//! LED : PF6（高电平 = 亮红灯 = 上电状态）
//!
//! LAN8720A PHY 地址: 0（PHYAD0 接 GND 时为 0，接 VCC 时为 1）
//!
//! Redfish 接口（HTTP 端口 80）：
//!   上电: POST /redfish/v1/Systems/1/Actions/ComputerSystem.Reset
//!         {"ResetType":"On"}
//!   断电: POST /redfish/v1/Systems/1/Actions/ComputerSystem.Reset
//!         {"ResetType":"ForceOff"}  或  {"ResetType":"GracefulShutdown"}
//!
//! 网络: DHCP 自动获取 IP

//! BMC (Baseboard Management Controller) - 带外管理第一阶段：上下电控制
//! 硬件: STM32F407ZG + LAN8720A (RMII)
//! BMC (Baseboard Management Controller)
//! 硬件: STM32F407ZG + LAN8720A (RMII)
#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_net::{Ipv4Cidr, StackResources, Ipv4Address, StaticConfigV4};
use embassy_stm32::{
    Config, bind_interrupts, eth::{self, Ethernet, GenericPhy, PacketQueue}, gpio::{Level, Output, Speed}, peripherals::ETH, time::Hertz
};
use embassy_stm32::rcc::*;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embassy_time::Duration;
use heapless::Vec;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
});

type Device = Ethernet<'static, ETH, GenericPhy>;

static POWER_SIGNAL: Signal<ThreadModeRawMutex, bool> = Signal::new();
static PACKETS: StaticCell<PacketQueue<4, 4>> = StaticCell::new();
static NET_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Device>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn led_task(mut led: Output<'static>) -> ! {
    loop {
        let power_on = POWER_SIGNAL.wait().await;
        if power_on {
            led.set_low(); 
            info!("[BMC] Power ON -> LED Lit");
        } else {
            led.set_high();
            info!("[BMC] Power OFF -> LED Off");
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // 1. 时钟配置
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

    // 2. LED
    let led = Output::new(p.PF6, Level::High, Speed::Low);

    // 3. MAC 地址
    let mac_addr = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];

    // 4. 以太网初始化
    let device = Ethernet::new(
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
        // 【修正4】这里使用 GenericPhy::new(0)
        GenericPhy::new(0), 
        mac_addr,
    );

    // 使用静态ip
    // 设置ip
    let address = Ipv4Address::new(192, 168, 2, 177);
    let cidr = Ipv4Cidr::new(address, 24);

    // 设置网关
    let gateway = Ipv4Address::new(192, 168, 2, 1);

    // 设置dns服务器（这里留空）
    let dns_servers: Vec<Ipv4Address, 3> = Vec::new();

    let static_config = StaticConfigV4 {
        address: cidr,
        gateway: Some(gateway),
        dns_servers: dns_servers,
    };
    let config = embassy_net::Config::ipv4_static(static_config);

    let (stack, net_runner) = embassy_net::new(
        device,
        config,
        NET_RESOURCES.init(StackResources::new()),
        0x1234_5678,
    );

    spawner.spawn(net_task(net_runner)).unwrap();
    spawner.spawn(led_task(led)).unwrap();

    info!("network initialized using static ip: {}", address);
    stack.wait_config_up().await;

    let ip = stack.config_v4().unwrap().address;
    info!("IP Address: {}", ip);
    info!("Try: curl -X POST http://{}/redfish/v1/Systems/1/Actions/ComputerSystem.Reset -d '{{\"PowerOn\"}}'", ip.address());

    loop {
        if stack.is_link_up() {
            info!("LINK UP");
            break;
        } else {
            warn!("LINK DOWN");
        }
        embassy_time::Timer::after_secs(1).await;
    }

    // ── HTTP Server Loop ──
    let mut rx_buf = [0u8; 1024];
    let mut tx_buf = [0u8; 1024];

    loop {
        let mut socket = embassy_net::tcp::TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(Duration::from_secs(15)));

        if let Err(e) = socket.accept(80).await {
            warn!("Accept error: {:?}", e);
            continue;
        }

        handle_request(&mut socket).await;
    }
}

async fn handle_request(socket: &mut embassy_net::tcp::TcpSocket<'_>) {
    let mut buf = [0u8; 1024];
    let mut filled = 0usize;

    // 读取 Header
    'read: loop {
        match socket.read(&mut buf[filled..]).await {
            Ok(0) | Err(_) => break 'read,
            Ok(n) => {
                filled += n;
                if buf[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
                    break 'read;
                }
                if filled >= buf.len() { break 'read; }
            }
        }
    }

    let req_str = core::str::from_utf8(&buf[..filled]).unwrap_or("");
    let mut method = "";
    let mut path = "";
    if let Some(line) = req_str.lines().next() {
        let mut parts = line.split_whitespace();
        method = parts.next().unwrap_or("");
        path = parts.next().unwrap_or("");
    }

    match (method, path) {
        // health check
        ("GET", "/ping") => {
            send_response(socket, 200, b"OK", b"connection up!").await;
        }
        // Redfish root
        ("GET", "/redfish/v1") => {
            send_response(socket, 200, b"OK", ROOT_RESOURCE.as_bytes()).await;
        }
        // Redfish Systems collection
        ("GET", "/redfish/v1/Systems") => {
            send_response(socket, 200, b"OK", SYSTEM_RESOURCE.as_bytes()).await;
        }
        // Redfish Systems, return power state
        ("GET", "/redfish/v1/Systems/1") => {
            send_response(socket, 200, b"OK", dump_system_info("1").await.as_bytes()).await;
        }
        // power control
        ("POST", "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset") => {
            if req_str.contains("\"ResetType\":\"On\"") {
                POWER_SIGNAL.signal(true);
                send_response(socket, 200, b"OK", b"Power On!").await;
            } else if req_str.contains("\"ResetType\":\"ForceOff\"") {
                POWER_SIGNAL.signal(false);
                send_response(socket, 200, b"OK", b"Force Power Off!").await;
            } else {
                send_response(socket, 404, b"Not Found", b"").await;
            }
        }

        _ => {
            send_response(socket, 404, b"Not Found", b"").await;
        }
    }
    
    // Responses are handled in the match above; avoid duplicate handling here.

    let _ = socket.flush().await;
    socket.close();
}

async fn send_response(
    socket: &mut embassy_net::tcp::TcpSocket<'_>,
    status: u16,
    reason: &[u8],
    body: &[u8],
) {
    use embedded_io_async::Write;
    let mut num_buf = [0u8; 8];
    
    let _ = socket.write_all(b"HTTP/1.1 ").await;
    let _ = socket.write_all(itoa(status, &mut num_buf)).await;
    let _ = socket.write_all(b" ").await;
    let _ = socket.write_all(reason).await;
    let _ = socket.write_all(b"\r\nContent-Type: application/json\r\nContent-Length: ").await;
    let _ = socket.write_all(itoa(body.len() as u16, &mut num_buf)).await;
    let _ = socket.write_all(b"\r\nConnection: close\r\n\r\n").await;
    if !body.is_empty() {
        let _ = socket.write_all(body).await;
    }
}

fn itoa(mut n: u16, buf: &mut [u8]) -> &[u8] {
    if n == 0 { return b"0"; }
    let mut i = 0;
    while n > 0 && i < buf.len() {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    let slice = &mut buf[..i];
    slice.reverse();
    slice
}

static ROOT_RESOURCE: &str = r##"{
    "@odata.type": "#ServiceRoot.v1_15_0.ServiceRoot",
    "@odata.id": "/redfish/v1",
    "@odata.context": "/redfish/v1/$metadata#ServiceRoot.ServiceRoot",
    "Id": "RootService",
    "Name": "Root Service",
    "RedfishVersion": "1.13.0",
    "Systems": {
        "@odata.id": "/redfish/v1/Systems"
    },
    "Chassis": {
        "@odata.id": "/redfish/v1/Chassis"
    },
    "Managers": {
        "@odata.id": "/redfish/v1/Managers"
    }
}"##;

static SYSTEM_RESOURCE: &str = r##"{
  "@odata.id": "/redfish/v1/Systems",
  "Members": [
    { "@odata.id": "/redfish/v1/Systems/1" }
  ],
  "Members@odata.count": 1
}"##;

async fn dump_system_info(_system_id: &str) -> &'static str {
    r##"{
        "@odata.type": "#ComputerSystem.v1_15_0.ComputerSystem",
        "@odata.id": "/redfish/v1/Systems/1",
        "Id": "1",
        "Name": "Main System",
        "PowerState": "On",
        "Actions": {
            "#ComputerSystem.Reset": {
                "target": "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
                "ResetType@Redfish.AllowableValues": [
                    "On",
                    "ForceOff",
                ]
            }
        }
    }"##
}
