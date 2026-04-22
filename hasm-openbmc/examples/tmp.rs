//! 接线说明
//! 树莓派 UART0 TXD -> STM32 PA10 (USART1_RX)
//! 树莓派 UART0 RXD -> STM32 PA9 (USART1_TX)
//! 树莓派 GND -> STM32 GND
//! 树莓派与stm32需在同一局域网，本例程stm32静态ip为192.168.1.177
#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_net::Ipv4Address;
use embassy_stm32::eth;
use hasm_openbmc::{
    config::get_ip, consts::UART_BAUDRATE, drivers::{ethernet::ethernet_device, uart::uart_init}, hal::init::sys_init, net::{init_eth_stack, net_task}, services::console::console_task
};
use {defmt_rtt as _, panic_probe as _};


#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = sys_init();

    info!("UART bridge booting...");

    let eth_dev = ethernet_device(p.ETH, p.PA1, p.PA2, p.PC1, p.PA7, p.PC4, p.PC5, p.PG13, p.PG14, p.PG11);
    let (stack, runner) = init_eth_stack(eth_dev);

    unwrap!(spawner.spawn(net_task(runner)));
    let IP_ADDR = get_ip();
    info!("network configured: {}", IP_ADDR);
    stack.wait_config_up().await;

    while !stack.is_link_up() {
        warn!("ethernet link is not ready, retrying...");
        embassy_time::Timer::after_secs(1).await;
    }

    let uart = uart_init(p.USART1, p.PA10, p.PA9, UART_BAUDRATE);
    info!("UART ready: Raspberry Pi TXD -> STM32 PA10 (USART1_RX)");
    info!("UART ready: optional Raspberry Pi RXD -> STM32 PA9 (USART1_TX)");
    info!("UART ready: open tcp://{}:2323 before powering on the Raspberry Pi", IP_ADDR);

    unwrap!(spawner.spawn(console_task(uart, stack)));
}