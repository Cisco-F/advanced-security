#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Speed};
use hasm_openbmc::{drivers::{ethernet::ethernet_device, led::{led_init, led_task}}, hal::init::sys_init, net::{init_eth_stack, net_task}, services::{power_control::{PowerControl, power_task}, web_server::http_task}};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = sys_init();

    let power_control = PowerControl::new(p.PB3, p.PB4);

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
    spawner.spawn(net_task(runner)).unwrap();

    let led = led_init(p.PF6, Level::Low, Speed::Low);

    let _ = spawner.spawn(http_task(stack));
    let _ = spawner.spawn(power_task(power_control));
    let _ = spawner.spawn(led_task(led));
}
