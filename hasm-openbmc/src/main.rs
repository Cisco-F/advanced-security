#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts, eth::{self, Ethernet, GenericPhy}, gpio::{Level, Speed}, peripherals::ETH
};
use {defmt_rtt as _, panic_probe as _};

use hasm_openbmc::{config::get_ip, consts::UART_BAUDRATE, drivers::{ethernet::ethernet_device, led::{led_init, led_task}, uart::uart_init}, hal::init::sys_init, net::init_eth_stack, services::{console::console_task, power_control::{PowerControl, power_task}, web_server::http_task}};


#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Ethernet<'static, ETH, GenericPhy>>) -> ! {
    runner.run().await
}

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
    unwrap!(spawner.spawn(net_task(runner)));

    stack.wait_config_up().await;
    
    // 串口控制初始化
    let ip = get_ip();
    let uart = uart_init(p.USART1, p.PA10, p.PA9, UART_BAUDRATE);
    info!("UART ready: Raspberry Pi TXD -> STM32 PA10 (USART1_RX)");
    info!("UART ready: optional Raspberry Pi RXD -> STM32 PA9 (USART1_TX)");
    info!("UART ready: open tcp://{}:2323 before powering on the Raspberry Pi", ip);

    unwrap!(spawner.spawn(console_task(uart, stack)));

    // 电源管理模块初始化
    let power_control = PowerControl::new(p.PB3, p.PB4);
    let led = led_init(p.PF6, Level::Low, Speed::Low);

    let _ = unwrap!(spawner.spawn(http_task(stack)));
    let _ = unwrap!(spawner.spawn(power_task(power_control)));
    let _ = unwrap!(spawner.spawn(led_task(led)));

}
