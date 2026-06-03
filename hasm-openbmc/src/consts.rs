//! Board-local configuration constants.
//!
//! These values describe the default lab topology: the STM32 board is
//! `169.254.77.2`, the host PC serving disk-image ranges is `169.254.77.1`,
//! and both sides sit on a link-local direct-attach network. Keeping these as
//! plain constants avoids flash/RAM cost for runtime configuration on the
//! firmware.
//!
//! The MAC address uses a locally administered prefix (`0x02`) so it should not
//! collide with vendor-assigned addresses. Change it if multiple boards share
//! the same Ethernet segment.

/// Static management address of the STM32 BMC.
pub const BOARD_IP: [u8; 4] = [169, 254, 77, 2];
/// Host PC address for the HTTP range image server.
pub const HOST_IP: [u8; 4] = [169, 254, 77, 1];
/// IPv4 prefix length for the link-local lab subnet.
pub const PREFIX: u8 = 16;
/// Default gateway kept for completeness when the board is put on a routed LAN.
pub const GATEWAY: [u8; 4] = [169, 254, 77, 1];
/// Locally administered MAC address used by the STM32 Ethernet peripheral.
pub const MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];

/// UART speed expected by the Raspberry Pi serial console.
pub const UART_BAUDRATE: u32 = 115_200;
/// TCP port that exposes the UART bridge to the host.
pub const TELNET_PORT: u16 = 2323;
/// TCP port of the Python image server running on the host PC.
pub const IMG_SERVER_PORT: u16 = 8000;
