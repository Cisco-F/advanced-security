#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::eth::{Ethernet, GenericPhy};
use embassy_time::{Timer, Duration};
use hasm_openbmc::{drivers::ethernet::ethernet_device, hal::init::sys_init, net::init_eth_stack};
use {defmt_rtt as _, panic_probe as _};


type EthDevice = Ethernet<'static, embassy_stm32::peripherals::ETH, GenericPhy>;

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, EthDevice>) -> ! {
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = sys_init();
    info!("LAN8720 test start");

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

    loop {
        info!("link: {}", stack.is_link_up());
        Timer::after(Duration::from_secs(1)).await;
    }
}