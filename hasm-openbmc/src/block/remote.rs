//! Remote HTTP-backed block device.
//!
//! The Python host script exposes a disk image through `/image` and supports
//! HTTP `Range: bytes=start-end` requests. This module translates USB MSC sector
//! reads into those byte ranges so the STM32 can boot the Raspberry Pi from an
//! image file stored on the development machine.
//!
//! The implementation opens a fresh TCP connection for each `read_blocks` call.
//! That is not the most bandwidth-efficient design, but it is robust for a small
//! lab BMC: connection lifetime is short, socket state is simple, and the cache
//! layer above this backend removes most tiny repeated reads.
//!
//! The backend intentionally ignores HTTP status parsing beyond locating the end
//! of the header. The paired server is controlled by the same project and always
//! returns the requested bytes or closes the connection on error. Any premature
//! close is treated as a block-read failure.
//!
//! All numeric formatting is done into stack buffers to avoid `alloc` and keep
//! the firmware compatible with `no_std`.

use defmt::*;
use crate::{block::BlockDevice, utils::{format_ip, u32_to_ascii}};

use {defmt_rtt as _, panic_probe as _};
use embassy_net::{tcp::TcpSocket, Ipv4Address, Stack};
use embedded_io_async::Write;
use panic_probe as _;

/// Block backend that retrieves sectors from a host-side HTTP image server.
pub struct RemoteBlockDevice {
    /// Shared Embassy network stack handle.
    stack: Stack<'static>,
    /// Host PC serving the image.
    server: Ipv4Address,
    /// TCP port of the image server.
    port: u16,
}

impl RemoteBlockDevice {
    /// Create a backend pointed at `server:port`.
    pub fn new(stack: Stack<'static>, server: Ipv4Address, port: u16) -> Self {
        Self { stack, server, port }
    }
}

impl BlockDevice for RemoteBlockDevice {
    async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        self.read_blocks(lba, buf).await?;
        Ok(())
    }

    async fn read_blocks(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        let total_bytes = buf.len();
        if total_bytes == 0 {
            return Ok(());
        }

        let mut socket_rx_buffer = [0u8; 1024];
        let mut socket_tx_buffer = [0u8; 512];
        let mut socket = TcpSocket::new(self.stack, &mut socket_rx_buffer, &mut socket_tx_buffer);

        let start_byte = lba * 512;
        let end_byte = start_byte + total_bytes as u32 - 1;

        // HTTP byte ranges are inclusive, matching the `end_byte` expression
        // above. A one-sector read at LBA 0 therefore asks for bytes 0..511.
        let remote_endpoint = (self.server, self.port);
        debug!("RemoteImage: GET /image Range={}..{} -> connecting", start_byte, end_byte);
        if let Err(e) = socket.connect(remote_endpoint).await {
            warn!("connect error: {:?}", e);
            return Err(());
        }
        debug!("RemoteImage: connected to server");

        let mut numbuf = [0u8; 24];

        // Build the request in small writes so no heap-backed formatted string
        // is needed. The server accepts HTTP/1.1 plus a Host header and closes
        // the connection after the response body.
        let _ = socket.write_all(b"GET /image HTTP/1.1\r\nHost: ").await;
        let mut ip_buf = [0u8; 16];
        let s = format_ip(self.server, &mut ip_buf);
        let _ = socket.write_all(s).await;
        let _ = socket.write_all(b"\r\nRange: bytes=").await;
        
        let start_ascii = u32_to_ascii(start_byte, &mut numbuf);
        let _ = socket.write_all(start_ascii).await;
        let _ = socket.write_all(b"-").await;
        let end_ascii = u32_to_ascii(end_byte, &mut numbuf);
        let _ = socket.write_all(end_ascii).await;
        let _ = socket.write_all(b"\r\nConnection: close\r\n\r\n").await;

        // Read the HTTP header until CRLF CRLF. Any body bytes that arrive in
        // the same TCP packet are preserved below instead of being discarded.
        let mut header_buf = [0u8; 512];
        let mut filled = 0usize;
        'read_hdr: loop {
            match socket.read(&mut header_buf[filled..]).await {
                Ok(0) => { warn!("socket closed while reading header"); return Err(()); }
                Err(_) => { warn!("socket read error while reading header"); return Err(()); }
                Ok(n) => {
                    filled += n;
                    if header_buf[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
                        break 'read_hdr;
                    }
                    if filled >= header_buf.len() { warn!("header too large"); return Err(()); }
                }
            }
        }

        // Find the first byte after the header terminator.
        let mut body_start = 0usize;
        for i in 0..filled - 3 {
            if &header_buf[i..i + 4] == b"\r\n\r\n" {
                body_start = i + 4;
                break;
            }
        }

        // Copy any body bytes that were coalesced with the header read.
        let mut got = 0usize;
        if filled > body_start {
            let mut avail = filled - body_start;
            let mut src_off = body_start;
            while avail > 0 && got < total_bytes {
                let take = core::cmp::min(total_bytes - got, avail);
                buf[got..got + take].copy_from_slice(&header_buf[src_off..src_off + take]);
                src_off += take;
                avail -= take;
                got += take;
            }
        }

        // Continue reading until the caller's buffer contains the complete
        // sector range requested by the SCSI layer.
        while got < total_bytes {
            let dst = &mut buf[got..];
            match socket.read(dst).await {
                Ok(0) => { warn!("socket closed while reading body"); return Err(()); }
                Err(_) => { warn!("socket read error while reading body"); return Err(()); }
                Ok(n) => got += n,
            }
        }

        let _ = socket.flush().await;
        socket.close();
        // Log after the buffer is filled so an operator can correlate successful
        // SCSI reads with the Python server's Range log.
        info!("RemoteImage: fetched byte range {}..{} ({} bytes, LBA {})", start_byte, end_byte, total_bytes, lba);

        Ok(())
    }
}
