use defmt::*;
use crate::{block::BlockDevice, utils::{format_ip, u32_to_ascii}};

use {defmt_rtt as _, panic_probe as _};
use embassy_net::{tcp::TcpSocket, Ipv4Address, Stack};
use embedded_io_async::Write;
use panic_probe as _;

pub struct RemoteBlockDevice {
    stack: Stack<'static>,
    server: Ipv4Address,
    port: u16,
}

impl RemoteBlockDevice {
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

        let remote_endpoint = (self.server, self.port);
        debug!("RemoteImage: GET /image Range={}..{} -> connecting", start_byte, end_byte);
        if let Err(e) = socket.connect(remote_endpoint).await {
            warn!("connect error: {:?}", e);
            return Err(());
        }
        debug!("RemoteImage: connected to server");

        let mut numbuf = [0u8; 24];

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

        // read header
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

        // find start of body
        let mut body_start = 0usize;
        for i in 0..filled - 3 {
            if &header_buf[i..i + 4] == b"\r\n\r\n" {
                body_start = i + 4;
                break;
            }
        }

        // copy any body bytes already read into blocks sequentially
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

        // continue reading until we filled all requested bytes
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
        info!("RemoteImage: fetched byte range {}..{} ({} bytes, LBA {})", start_byte, end_byte, total_bytes, lba);

        Ok(())
    }
}