use core::net::Ipv4Addr;
use embassy_net::{Ipv4Cidr, StaticConfigV4};
use heapless::Vec;

use crate::consts::{BOARD_IP, GATEWAY, HOST_IP, PREFIX};

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
        dns_servers: Vec::new(), // 可以添加DNS服务器地址
    };
    embassy_net::Config::ipv4_static(static_config)
}

pub fn get_board_ip() -> Ipv4Addr {
    Ipv4Addr::new(
        BOARD_IP[0],
        BOARD_IP[1],
        BOARD_IP[2],
        BOARD_IP[3]
    )
}

pub fn get_host_ip() -> Ipv4Addr {
    Ipv4Addr::new(
        HOST_IP[0],
        HOST_IP[1],
        HOST_IP[2],
        HOST_IP[3]
    )
}