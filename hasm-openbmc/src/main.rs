#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts, 
    eth::{self, Ethernet, GenericPhy}, 
    peripherals::ETH
};
use {defmt_rtt as _, panic_probe as _};

use hasm_openbmc::{drivers::ethernet::ethernet_device, hal::init::sys_init, net::init_eth_stack};


bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
});

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
    spawner.spawn(net_task(runner)).unwrap();

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

}
