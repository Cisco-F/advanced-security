use embassy_net::{Runner, Stack, StackResources};
use embassy_stm32::{eth::{Ethernet, GenericPhy}, peripherals::ETH};
use static_cell::StaticCell;

use crate::config::static_ipv4_config;


static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

pub fn init_eth_stack(device: Ethernet<'static, ETH, GenericPhy>) -> (
    Stack<'static>,
    Runner<'static, Ethernet<'static, ETH, GenericPhy>>
) {
    let (stack, runner) = embassy_net::new(
        device,
        static_ipv4_config(),
        RESOURCES.init(StackResources::new()),
        0x12345678,
    );

    (stack, runner)
}