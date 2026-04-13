//! 接线说明
//! 树莓派 UART0 TXD -> STM32 PA10 (USART1_RX)
//! 树莓派 UART0 RXD -> STM32 PA9 (USART1_TX)
//! 树莓派 GND -> STM32 GND
//! 树莓派与stm32需在同一局域网，本例程stm32静态ip为192.168.1.177
#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_net::{tcp::TcpSocket, Ipv4Address, Ipv4Cidr, StackResources, StaticConfigV4};
use embassy_stm32::{
    bind_interrupts,
    eth::{self, Ethernet, GenericPhy, PacketQueue},
    peripherals::{ETH, USART1},
    rcc::*,
    time::Hertz,
    usart::{self, BufferedUart, Config as UartConfig},
    Config as StmConfig,
};
use embedded_io_async::{Read, Write};
use heapless::Vec;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
    USART1 => usart::BufferedInterruptHandler<USART1>;
});

static PACKETS: StaticCell<PacketQueue<4, 4>> = StaticCell::new();
static NET_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
static UART_TX_BUF: StaticCell<[u8; 256]> = StaticCell::new();
static UART_RX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();

const IP_ADDR: Ipv4Address = Ipv4Address::new(192, 168, 1, 177);
const GATEWAY: Ipv4Address = Ipv4Address::new(192, 168, 1, 1);
const UART_BAUDRATE: u32 = 115_200;
const CONSOLE_PORT: u16 = 2323;

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Ethernet<'static, ETH, GenericPhy>>) -> ! {
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
            divp: Some(PllPDiv::DIV2),
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }
    let p = embassy_stm32::init(config);

    info!("UART bridge booting...");

    let mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    let device = Ethernet::new(
        PACKETS.init(PacketQueue::new()),
        p.ETH,
        Irqs,
        p.PA1,
        p.PA2,
        p.PC1,
        p.PA7,
        p.PC4,
        p.PC5,
        p.PG13,
        p.PG14,
        p.PG11,
        GenericPhy::new(0),
        mac,
    );

    let cidr = Ipv4Cidr::new(IP_ADDR, 24);
    let dns_servers: Vec<Ipv4Address, 3> = Vec::new();
    let static_config = StaticConfigV4 {
        address: cidr,
        gateway: Some(GATEWAY),
        dns_servers,
    };
    let net_config = embassy_net::Config::ipv4_static(static_config);
    let (stack, runner) = embassy_net::new(
        device,
        net_config,
        NET_RESOURCES.init(StackResources::new()),
        0x1234_5678,
    );

    unwrap!(spawner.spawn(net_task(runner)));
    info!("network configured: {}", IP_ADDR);
    stack.wait_config_up().await;

    while !stack.is_link_up() {
        warn!("ethernet link is not ready, retrying...");
        embassy_time::Timer::after_secs(1).await;
    }

    let mut uart_cfg = UartConfig::default();
    uart_cfg.baudrate = UART_BAUDRATE;

    let mut uart = unwrap!(BufferedUart::new(
        p.USART1,
        p.PA10,
        p.PA9,
        UART_TX_BUF.init([0; 256]),
        UART_RX_BUF.init([0; 1024]),
        Irqs,
        uart_cfg,
    ));

    info!("UART ready: Raspberry Pi TXD -> STM32 PA10 (USART1_RX)");
    info!("UART ready: optional Raspberry Pi RXD -> STM32 PA9 (USART1_TX)");
    info!("UART ready: open tcp://{}:{} before powering on the Raspberry Pi", IP_ADDR, CONSOLE_PORT);

    let mut socket_rx_buffer = [0u8; 1024];
    let mut socket_tx_buffer = [0u8; 1024];

    loop {
        let mut socket = TcpSocket::new(stack, &mut socket_rx_buffer, &mut socket_tx_buffer);
        socket.set_keep_alive(Some(embassy_time::Duration::from_secs(10)));

        info!("UART console listening on {}:{}", IP_ADDR, CONSOLE_PORT);
        if let Err(e) = socket.accept(CONSOLE_PORT).await {
            warn!("accept error: {:?}", e);
            continue;
        }

        info!("console client connected: {:?}", socket.remote_endpoint());

        if let Err(e) = socket
            .write_all(b"STM32 UART bridge connected. Waiting for Raspberry Pi boot log...\r\n")
            .await
        {
            warn!("banner write error: {:?}", e);
            socket.abort();
            let _ = socket.flush().await;
            continue;
        }

        match bridge_uart_to_socket(&mut uart, &mut socket).await {
            Ok(()) => info!("console client closed"),
            Err(()) => warn!("console session ended"),
        }

        socket.abort();
        let _ = socket.flush().await;
    }
}

async fn bridge_uart_to_socket<'d>(
    uart: &mut BufferedUart<'d>,
    socket: &mut TcpSocket<'_>,
) -> Result<(), ()> {
    let mut uart_buf = [0u8; 128];

    loop {
        let count = match uart.read(&mut uart_buf).await {
            Ok(count) => count,
            Err(e) => {
                warn!("uart read error: {:?}", e);
                return Err(());
            }
        };

        if count == 0 {
            continue;
        }

        if let Err(e) = socket.write_all(&uart_buf[..count]).await {
            warn!("tcp write error: {:?}", e);
            return Err(());
        }
    }
}