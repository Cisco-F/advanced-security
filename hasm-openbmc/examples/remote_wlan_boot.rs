#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts,
    peripherals,
    sdmmc::DataBlock,
    usb::{self, Config as UsbConfig, Driver},
};
use embassy_usb::Builder;
use embassy_usb::Handler;
use embassy_usb::control::{InResponse, OutResponse, Recipient, Request, RequestType};
use {defmt_rtt as _, panic_probe as _};
use embassy_time::Timer;
use embassy_usb::driver::{EndpointIn, EndpointOut};
use embassy_net::{tcp::TcpSocket, Ipv4Address, Ipv4Cidr, StackResources, StaticConfigV4, Stack};
use embassy_stm32::{
    eth::{self, Ethernet, GenericPhy, PacketQueue},
};
use static_cell::StaticCell;
use panic_probe as _;

use hasm_openbmc::scsi::*;


static EP_OUT_BUFFER: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static CONFIG_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static BOS_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static CTRL_BUF: static_cell::StaticCell<[u8; 64]> = static_cell::StaticCell::new();
static MSC_HANDLER: static_cell::StaticCell<MscHandler> = static_cell::StaticCell::new();
// 16 扇区 × 512 = 8192 字节；DataBlock 是 DMA 安全的 4 字节对齐块，放在 BSS 段避免栈溢出
// 24 扇区 × 512 = 12288 字节 (12 KiB)。DataBlock 是 DMA 安全的 4 字节对齐块，放在 BSS 段避免栈溢出
static CACHE_BUF: static_cell::StaticCell<[DataBlock; 24]> = static_cell::StaticCell::new();

/// USB MSC class-specific control request handler.
struct MscHandler {
    iface_num: u8,
}

impl Handler for MscHandler {
    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        if req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.request == 0xFE
            && req.index == self.iface_num as u16
        {
            info!("GET_MAX_LUN -> 0");
            buf[0] = 0x00;
            Some(InResponse::Accepted(&buf[..1]))
        } else {
            None
        }
    }

    fn control_out(&mut self, req: Request, _data: &[u8]) -> Option<OutResponse> {
        if req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.request == 0xFF
            && req.index == self.iface_num as u16
        {
            info!("BULK_ONLY_RESET");
            Some(OutResponse::Accepted)
        } else {
            None
        }
    }
}

/// Remote image fetcher abstraction.
///
/// This provides a small async API to read one-or-more 512-byte sectors (LBA) from
/// a remote HTTP server. The actual network implementation is left as TODO —
/// currently this stub fills the buffer with zeros so the rest of the USB MSC
/// logic can be tested and integrated.
static PACKETS: StaticCell<PacketQueue<4, 4>> = StaticCell::new();
static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

/// Remote image fetcher abstraction which holds a reference to the network `Stack`
/// and the server address. It performs simple HTTP GET requests per-block.
pub struct RemoteImage {
    stack: Stack<'static>,
    server: Ipv4Address,
    port: u16,
}

impl RemoteImage {
    pub fn new(stack: Stack<'static>, server: Ipv4Address, port: u16) -> Self {
        Self { stack, server, port }
    }

    /// Convert u32 to ASCII decimal into provided buffer, return slice.
    fn u32_to_ascii(mut n: u32, buf: &mut [u8]) -> &[u8] {
        if n == 0 { return b"0"; }
        let mut i = 0usize;
        while n > 0 && i < buf.len() {
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            i += 1;
        }
        buf[..i].reverse();
        &buf[..i]
    }

    /// Read multiple 512-byte blocks starting at `start_lba` into `buf`.
    ///
    /// This implementation performs one HTTP connection per requested block
    /// and expects the server to respond with the raw 512 bytes in the body.
    /// It is intentionally simple to keep RAM usage low on the STM32.
    pub async fn read_blocks(&mut self, start_lba: u32, buf: &mut [DataBlock]) -> Result<(), ()> {
        use embedded_io_async::Write;

        // Batch request: ask for contiguous range covering all requested blocks
        let blocks = buf.len();
        if blocks == 0 {
            return Ok(());
        }
        let start_byte: u32 = start_lba.checked_mul(512u32).unwrap_or(0u32);
        let total_bytes_usize = blocks * 512usize;
        let total_bytes_u32 = total_bytes_usize as u32;

        // socket buffers on stack
        let mut socket_rx_buffer = [0u8; 1024];
        let mut socket_tx_buffer = [0u8; 512];
        let mut socket = TcpSocket::new(self.stack, &mut socket_rx_buffer, &mut socket_tx_buffer);

        let remote_endpoint = (self.server, self.port);
        debug!("RemoteImage: GET /image Range={}..{} -> connecting", start_byte, start_byte + total_bytes_u32 - 1);
        if let Err(e) = socket.connect(remote_endpoint).await {
            warn!("connect error: {:?}", e);
            return Err(());
        }
        debug!("RemoteImage: connected to server");

        // Build request: GET /image HTTP/1.1 with Range header
        let mut numbuf = [0u8; 24];
        let start_ascii = Self::u32_to_ascii(start_byte, &mut numbuf);

        let _ = socket.write_all(b"GET /image HTTP/1.1\r\nHost: ").await;
        let mut ip_buf = [0u8; 16];
        let s = format_ip(self.server, &mut ip_buf);
        let _ = socket.write_all(s).await;
        let _ = socket.write_all(b"\r\nRange: bytes=").await;
        let _ = socket.write_all(start_ascii).await;
        let _ = socket.write_all(b"-").await;
        let end_ascii = Self::u32_to_ascii(start_byte + total_bytes_u32 - 1, &mut numbuf);
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
            while avail > 0 && got < total_bytes_usize {
                let block_idx = got / 512;
                let byte_in_block = got % 512;
                let take = core::cmp::min(512 - byte_in_block, avail);
                let dst = &mut buf[block_idx].0[byte_in_block..byte_in_block + take];
                dst.copy_from_slice(&header_buf[src_off..src_off + take]);
                src_off += take;
                avail -= take;
                got += take;
            }
        }

        // continue reading until we filled all requested bytes
        while got < total_bytes_usize {
            let block_idx = got / 512;
            let byte_in_block = got % 512;
            let dst = &mut buf[block_idx].0[byte_in_block..];
            match socket.read(dst).await {
                Ok(0) => { warn!("socket closed while reading body"); return Err(()); }
                Err(_) => { warn!("socket read error while reading body"); return Err(()); }
                Ok(n) => got += n,
            }
        }

        let _ = socket.flush().await;
        socket.close();
        info!("RemoteImage: fetched LBA {}..{} ({} bytes)", start_lba, start_lba + blocks as u32 - 1, total_bytes_usize);

        Ok(())
    }
}

fn format_ip(ip: Ipv4Address, out: &mut [u8]) -> &[u8] {
    // write dotted quad into out, return slice
    let octets = ip.octets();
    let mut idx = 0usize;
    for (i, &o) in octets.iter().enumerate() {
        // write decimal
        let mut tmp = [0u8; 3];
        let mut n = o as u16;
        if n == 0 {
            out[idx] = b'0';
            idx += 1;
        } else {
            let mut t = 0usize;
            while n > 0 {
                tmp[t] = b'0' + (n % 10) as u8;
                n /= 10;
                t += 1;
            }
            for k in 0..t { out[idx + k] = tmp[t - 1 - k]; }
            idx += t;
        }
        if i != 3 {
            out[idx] = b'.';
            idx += 1;
        }
    }
    &out[..idx]
}

bind_interrupts!(struct UsbIrqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
});

type Device = Ethernet<'static, embassy_stm32::peripherals::ETH, GenericPhy>;

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Device>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>) {
    usb.run().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(hasm_openbmc::clk_init());

    info!("Remote WLAN Boot init...");

    // --- Network setup (static IP) ---
    const STM_IP: Ipv4Address = Ipv4Address::new(192, 168, 1, 177);
    const GATEWAY: Ipv4Address = Ipv4Address::new(192, 168, 1, 1);
    // 主机 HTTP 服务器地址（已按你要求设置为 192.168.1.77）
    const SERVER_IP: Ipv4Address = Ipv4Address::new(192, 168, 1, 77);
    const SERVER_PORT: u16 = 8000;

    let mac = [0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    let device = Ethernet::new(
        PACKETS.init(PacketQueue::new()),
        p.ETH,
        Irqs,

        // RMII pins (same as ethernet.rs)
        p.PA1,  // REFCLK
        p.PA2,  // MDIO
        p.PC1,  // MDC
        p.PA7,  // CRS_DV
        p.PC4,  // RXD0
        p.PC5,  // RXD1

        p.PG13, // TX_EN
        p.PG14, // TXD0
        p.PG11, // TXD1

        GenericPhy::new(0),
        mac,
    );

    let cidr = Ipv4Cidr::new(STM_IP, 24);
    let dns_servers: heapless::Vec<Ipv4Address, 3> = heapless::Vec::new();
    let static_config = StaticConfigV4 {
        address: cidr,
        gateway: Some(GATEWAY),
        dns_servers,
    };
    let net_config = embassy_net::Config::ipv4_static(static_config);

    let (stack, runner) = embassy_net::new(
        device,
        net_config,
        RESOURCES.init(StackResources::new()),
        0x1234_5678,
    );

    unwrap!(spawner.spawn(net_task(runner)));
    info!("network configured: {}", STM_IP);
    stack.wait_config_up().await;
    while !stack.is_link_up() {
        warn!("ethernet link is not ready, retrying...");
        Timer::after(embassy_time::Duration::from_secs(1)).await;
    }


    // USB 配置
    let ep_out_buffer = EP_OUT_BUFFER.init([0; 256]);
    let mut usb_cfg = UsbConfig::default();
    usb_cfg.vbus_detection = false;
    let driver = Driver::new_fs(
        p.USB_OTG_FS,
        UsbIrqs,
        p.PA12,
        p.PA11,
        ep_out_buffer,
        usb_cfg,
    );

    let config_desc = CONFIG_DESC.init([0; 256]);
    let bos_desc = BOS_DESC.init([0; 256]);
    let ctrl_buf = CTRL_BUF.init([0; 64]);

    let mut cfg = embassy_usb::Config::new(0xc0de, 0xcafe);
    cfg.manufacturer = Some("MyBMC");
    cfg.product = Some("STM32F407 USB MSC (remote)");
    cfg.serial_number = Some("F407-MSC-REMOTE-001");
    cfg.max_power = 100;
    cfg.max_packet_size_0 = 64;

    let mut builder = Builder::new(driver, cfg, config_desc, bos_desc, &mut [], ctrl_buf);

    let mut function = builder.function(0x08, 0x06, 0x50);
    let mut interface = function.interface();
    let iface_num = interface.interface_number().0;
    let mut alt_setting = interface.alt_setting(0x08, 0x06, 0x50, None);
    let mut ep_out = alt_setting.endpoint_bulk_out(None, 64);
    let mut ep_in = alt_setting.endpoint_bulk_in(None, 64);
    drop(function);

    let msc_handler = MSC_HANDLER.init(MscHandler { iface_num });
    builder.handler(msc_handler);

    let usb = builder.build();
    unwrap!(spawner.spawn(usb_task(usb)));

    info!("✓ USB MSC device ready (remote)");

    let mut cbw_buf = [0u8; 31];
    let mut meta_buf = [0u8; 64];

    // 缓存区：16 × 512 = 8192 字节
    // 缓存区：24 × 512 = 12288 字节 (单次最大传输 12 KiB)
    const CACHE_SECS: usize = 24;
    let cache_blocks: &mut [DataBlock; CACHE_SECS] =
        CACHE_BUF.init(core::array::from_fn(|_| DataBlock([0u8; 512])));

    // Remote image client
    let mut remote = RemoteImage::new(stack, SERVER_IP, SERVER_PORT);

    loop {
        let n = match ep_out.read(&mut cbw_buf).await {
            Ok(n) => n,
            Err(e) => {
                warn!("MSC OUT read error: {:?}", e);
                Timer::after_millis(10).await;
                continue;
            }
        };

        if n < 31 {
            warn!("Short CBW: {}", n);
            continue;
        }

        let sig = u32::from_le_bytes([cbw_buf[0], cbw_buf[1], cbw_buf[2], cbw_buf[3]]);
        if sig != CBW_SIGNATURE {
            warn!("Bad CBW signature: 0x{:08x}", sig);
            continue;
        }

        let tag = u32::from_le_bytes([cbw_buf[4], cbw_buf[5], cbw_buf[6], cbw_buf[7]]);
        let dtl = u32::from_le_bytes([cbw_buf[8], cbw_buf[9], cbw_buf[10], cbw_buf[11]]);
        let flags = cbw_buf[12];
        let cb_len = core::cmp::min(cbw_buf[14] as usize, 16);
        let cmd = &cbw_buf[15..15 + cb_len];
        let mut response = handle_scsi_cmd(cmd, &mut meta_buf);

        if (flags & 0x80) != 0 && dtl > 0 && response.resp_len > 0 {
            let send_len = core::cmp::min(response.resp_len, dtl as usize);
            let mut offset = 0;
            let mut write_ok = true;

            let start_lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);

            let mut cache_start_lba: u32 = u32::MAX;
            let mut cache_valid_secs: usize = 0;

            while offset < send_len {
                let chunk_size = core::cmp::min(send_len - offset, 64);
                let chunk_data: &[u8];

                if cmd[0] == SCSI_READ_10 {
                    let sector_off = (offset as u32) / SECTOR_SIZE;
                    let byte_in_sec = (offset as u32 % SECTOR_SIZE) as usize;
                    let target_lba = start_lba + sector_off;

                    if target_lba < cache_start_lba || target_lba >= cache_start_lba + cache_valid_secs as u32 {
                        let total_secs = ((send_len as u32) + SECTOR_SIZE - 1) / SECTOR_SIZE;
                        let remaining = total_secs - sector_off;
                        let to_read = core::cmp::min(remaining as usize, CACHE_SECS);
                        cache_start_lba = target_lba;
                        cache_valid_secs = 0;

                        match remote.read_blocks(target_lba, &mut cache_blocks[..to_read]).await {
                            Ok(_) => { cache_valid_secs = to_read; }
                            Err(_) => {
                                error!("Remote read_blocks LBA {} failed", target_lba);
                                response.status = ScsiStatus::ScsiFail;
                                write_ok = false;
                            }
                        }
                        if !write_ok { break; }
                    }

                    let block_idx = (target_lba - cache_start_lba) as usize;
                    chunk_data = &cache_blocks[block_idx].0[byte_in_sec..byte_in_sec + chunk_size];
                } else {
                    chunk_data = &meta_buf[offset..offset + chunk_size];
                }

                if let Err(e) = ep_in.write(chunk_data).await {
                    warn!("MSC IN write chunk error: {:?}", e);
                    response.residue = dtl.saturating_sub(offset as u32);
                    write_ok = false;
                    break;
                }
                offset += chunk_size;
            }

            if write_ok && (send_len as u32) < dtl {
                response.residue = dtl.saturating_sub(send_len as u32);
            }
        } else if dtl > 0 {
            let mut bytes_read = 0;
            let mut dump_buf = [0u8; 64];
            while bytes_read < dtl {
                match ep_out.read(&mut dump_buf).await {
                    Ok(n) => { bytes_read += n as u32; }
                    Err(e) => { warn!("MSC OUT drain error: {:?}", e); break; }
                }
            }

            if cmd[0] == SCSI_WRITE_10 {
                warn!("write protected: command denied");
                response.status = ScsiStatus::ScsiFail;
            }

            response.residue = dtl.saturating_sub(bytes_read);
        }

        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        csw[4..8].copy_from_slice(&tag.to_le_bytes());
        csw[8..12].copy_from_slice(&response.residue.to_le_bytes());
        csw[12] = response.status as u8;

        if let Err(e) = ep_in.write(&csw).await {
            warn!("MSC CSW write error: {:?}", e);
        }
    }
}
