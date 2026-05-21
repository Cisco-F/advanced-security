#![no_std]
#![no_main]

//! UART-over-TCP console example.
//!
//! Starts Ethernet and the UART bridge only. The host connects to
//! `tcp://BOARD_IP:2323` to validate Raspberry Pi serial wiring.

use defmt::*;
use embassy_executor::Spawner;
use {defmt_rtt as _, panic_probe as _};
use hasm_openbmc::{
    config::get_board_ip,
    consts::UART_BAUDRATE,
    drivers::{
        ethernet::ethernet_device,
        uart::uart_init,
    },
    hal::init::sys_init,
    net::{init_eth_stack, net_task},
    services::console::console_task
};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = sys_init();

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
    stack.wait_config_up().await;
    unwrap!(spawner.spawn(net_task(runner)));

    let ip = get_board_ip();
    let uart = uart_init(p.USART1, p.PA10, p.PA9, UART_BAUDRATE);
    info!("UART ready: Raspberry Pi TXD -> STM32 PA10 (USART1_RX)");
    info!("UART ready: optional Raspberry Pi RXD -> STM32 PA9 (USART1_TX)");
    info!("UART ready: open tcp://{}:{} before powering on the Raspberry Pi", ip, UART_BAUDRATE);

    unwrap!(spawner.spawn(console_task(uart, stack)));

    info!("✓ UART console ready!");

}
