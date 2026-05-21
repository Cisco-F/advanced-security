//! Embassy network stack glue.
//!
//! The `Stack` handle is cheap to copy and is passed into each TCP service. The
//! `Runner` owns the actual Ethernet polling machinery and therefore must live
//! inside its own never-returning task.
//!
//! `StackResources<3>` reserves socket metadata for the small set of services
//! used by this firmware. Increasing the number of concurrently accepted TCP
//! sessions requires revisiting this capacity and the per-service buffers.

use embassy_net::{Runner, Stack, StackResources};
use embassy_stm32::{eth::{Ethernet, GenericPhy}, peripherals::ETH};
use static_cell::StaticCell;

use crate::config::static_ipv4_config;

static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

#[embassy_executor::task]
pub async fn net_task(mut runner: embassy_net::Runner<'static, Ethernet<'static, ETH, GenericPhy>>) -> ! {
    // `run` drives ARP, IP, TCP timers, and Ethernet RX/TX. If this future stops
    // making progress, every higher-level service appears to hang.
    runner.run().await
}

/// Create the Embassy IPv4 stack and its runner from an initialized Ethernet
/// device.
///
/// The seed is fixed because this firmware is deployed on a trusted lab
/// network and does not rely on randomized ephemeral-port selection for
/// security. Changing it is harmless if multiple boards are tested together.
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
