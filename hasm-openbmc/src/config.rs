//! Static network configuration helpers.
//!
//! The firmware uses a fixed lab network layout rather than DHCP. This module
//! converts byte-array constants into Embassy network types and exposes the board
//! and host addresses to services that need to log or connect to them.

use core::net::Ipv4Addr;
use embassy_net::{Ipv4Cidr, StaticConfigV4};
use heapless::Vec;

use crate::consts::{BOARD_IP, GATEWAY, HOST_IP, PREFIX};

/// Build the static IPv4 configuration used by the on-board Ethernet stack.
///
/// The board is normally cabled directly to the host running
/// `remote_image_server.py`, so DHCP would only add boot-time uncertainty.
/// Keeping the address in `consts.rs` makes classroom/lab network changes easy
/// without touching Embassy's network initialization code.
///
/// DNS is intentionally empty. All services use literal IPv4 addresses and the
/// firmware does not need to spend RAM on resolver state.
pub fn static_ipv4_config() -> embassy_net::Config {
    let ip = Ipv4Addr::new(
        BOARD_IP[0],
        BOARD_IP[1],
        BOARD_IP[2],
        BOARD_IP[3]
    );
    let cidr = Ipv4Cidr::new(ip, PREFIX);
    let gateway = Ipv4Addr::new(
        GATEWAY[0],
        GATEWAY[1],
        GATEWAY[2],
        GATEWAY[3]
    );

    let static_config = StaticConfigV4 {
        address: cidr,
        gateway: Some(gateway),
        dns_servers: Vec::new(), // DNS servers can be added here if name resolution is needed.
    };
    embassy_net::Config::ipv4_static(static_config)
}

/// Return the management IP of the STM32 BMC.
///
/// This address is used in user-facing log messages so operators know which
/// TCP endpoint to open for the UART bridge or HTTP control API.
pub fn get_board_ip() -> Ipv4Addr {
    Ipv4Addr::new(
        BOARD_IP[0],
        BOARD_IP[1],
        BOARD_IP[2],
        BOARD_IP[3]
    )
}

/// Return the host PC address used by the remote image backend.
///
/// The firmware initiates outbound TCP connections to this address whenever the
/// USB MSC layer misses the local sector cache and needs image bytes.
pub fn get_host_ip() -> Ipv4Addr {
    Ipv4Addr::new(
        HOST_IP[0],
        HOST_IP[1],
        HOST_IP[2],
        HOST_IP[3]
    )
}
