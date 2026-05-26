#![no_std]
#![no_main]

//! Standalone VNC/RFB diagnostics example.
//!
//! Self-contained no-auth RFB 3.8 server that renders a synthetic framebuffer on
//! TCP port 5900. Useful as a visual Ethernet/protocol smoke test.

use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Ipv4Address, Ipv4Cidr, StackResources, StaticConfigV4};
use embassy_stm32::eth::{Ethernet, GenericPhy, PacketQueue};
use embassy_stm32::peripherals::ETH;
use embassy_stm32::time::Hertz;
use embassy_stm32::Config as StmConfig;
use embassy_stm32::rcc::*;
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read, Write};
use defmt::{info, warn, error};
use heapless::Vec;
use {defmt_rtt as _, panic_probe as _};

embassy_stm32::bind_interrupts!(struct Irqs {
    ETH => embassy_stm32::eth::InterruptHandler;
});
static PACKETS: static_cell::StaticCell<PacketQueue<4, 4>> = static_cell::StaticCell::new();
static RESOURCES: static_cell::StaticCell<StackResources<2>> = static_cell::StaticCell::new();

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Ethernet<'static, ETH, GenericPhy>>) -> ! {
    runner.run().await
}

const SCREEN_WIDTH: u16 = 200;
const SCREEN_HEIGHT: u16 = 200;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = StmConfig::default();
    {
        config.rcc.hse = Some(Hse {
            freq: Hertz(25_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV25,
            mul: PllMul::MUL336,
            divp: Some(PllPDiv::DIV2), 
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }
    let p = embassy_stm32::init(config);
    info!("Board Initialization ok");

    let mac =[0x02, 0x00, 0x00, 0x00, 0x00, 0x01];
    let device = Ethernet::new(
        PACKETS.init(PacketQueue::new()),
        p.ETH, 
        Irqs,
        p.PA1,
        p.PA2, 
        p.PC1, 
        p.PA7, 
        p.PC4, 
        p.PC5, 
        p.PG13, 
        p.PG14, 
        p.PG11,
        GenericPhy::new(0), 
        mac,
    );

    let address = Ipv4Address::new(192, 168, 10, 2);
    let cidr = Ipv4Cidr::new(address, 24);
    let gateway = Ipv4Address::new(192, 168, 10, 1);
    let dns_servers: Vec<Ipv4Address, 3> = Vec::new();

    let static_config = StaticConfigV4 {
        address: cidr,
        gateway: Some(gateway),
        dns_servers,
    };
    let config = embassy_net::Config::ipv4_static(static_config);

    let (stack, runner) = embassy_net::new(
        device, config, RESOURCES.init(StackResources::new()), 0x12345678,
    );
    spawner.spawn(net_task(runner)).unwrap();

    info!("Waiting for network link...");
    while !stack.is_link_up() {
        Timer::after(Duration::from_millis(500)).await;
    }
    info!("Network link is UP! IP Address ready.");

    let mut rx_buffer = [0; 4096]; 
    let mut tx_buffer =[0; 4096];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        info!("VNC Server waiting for connection on 192.168.10.2:5900 ...");
        
        if let Err(e) = socket.accept(5900).await {
            error!("Accept error => {:?}", e);
            continue;
        }

        info!("Client Connected! Endpoint: {:?}", socket.remote_endpoint());

        match handle_vnc_session(&mut socket).await {
            Ok(_) => info!("VNC session gracefully closed"),
            Err(_) => warn!("VNC session error/aborted"),
        }
        
        socket.abort();
        info!("Socket closed. Ready for next connection.");
        Timer::after(Duration::from_millis(500)).await;
    }
}

fn generate_gradient_pixel(x: u16, y: u16) -> [u8; 3] {
    let hue = ((x as u32 + y as u32) * 360 / (SCREEN_WIDTH as u32 + SCREEN_HEIGHT as u32)) as u16;
    let sat = 255;
    let val = 200;
    
    let h = (hue / 60) % 6;
    let c = ((val as u32 * sat as u32) / 255) as u8;
    let x_val = (c as u32 * (60 - ((hue % 60) as u32))) / 60;
    
    match h {
        0 => [c, x_val as u8, 0],
        1 => [x_val as u8, c, 0],
        2 =>[0, c, x_val as u8],
        3 => [0, x_val as u8, c],
        4 => [x_val as u8, 0, c],
        _ =>[c, 0, x_val as u8],
    }
}

async fn handle_vnc_session(socket: &mut TcpSocket<'_>) -> Result<(), ()> {
    socket.write_all(b"RFB 003.008\n").await.map_err(|_| ())?;
    let mut client_version = [0u8; 12];
    socket.read_exact(&mut client_version).await.map_err(|_| ())?;

    // No authentication; this example is only for isolated lab diagnostics.
    socket.write_all(&[1, 1]).await.map_err(|_| ())?;
    let mut sec_type =[0u8; 1];
    socket.read_exact(&mut sec_type).await.map_err(|_| ())?;
    if sec_type[0] != 1 { return Err(()); }
    socket.write_all(&[0, 0, 0, 0]).await.map_err(|_| ())?;

    let mut client_init = [0u8; 1];
    socket.read_exact(&mut client_init).await.map_err(|_| ())?;

    let mut server_init =[0u8; 32];
    server_init[0..2].copy_from_slice(&SCREEN_WIDTH.to_be_bytes());
    server_init[2..4].copy_from_slice(&SCREEN_HEIGHT.to_be_bytes());
    
    // PixelFormat: 32-bit BGRA
    server_init[4] = 32;     // bits-per-pixel
    server_init[5] = 24;     // depth
    server_init[6] = 0;      // big-endian
    server_init[7] = 1;      // true-color
    server_init[8..10].copy_from_slice(&(255u16).to_be_bytes());  
    server_init[10..12].copy_from_slice(&(255u16).to_be_bytes()); 
    server_init[12..14].copy_from_slice(&(255u16).to_be_bytes()); 
    server_init[14] = 16;    
    server_init[15] = 8;     
    server_init[16] = 0;     
    
    server_init[20..24].copy_from_slice(&(8u32).to_be_bytes());
    server_init[24..32].copy_from_slice(b"STM32VNC");
    
    socket.write_all(&server_init).await.map_err(|_| ())?;
    info!("✓ Server Init Sent. Entering Event Loop!");

    let mut current_bpp = 32u8;

    loop {
        let mut msg_type = [0u8; 1];
        if socket.read_exact(&mut msg_type).await.is_err() {
            warn!("⚠️  Failed to read message type, connection lost");
            return Err(());
        }

        match msg_type[0] {
            // Set pixel format.
            0 => {
                let mut payload = [0u8; 19];
                match socket.read_exact(&mut payload).await {
                    Ok(_) => {
                        current_bpp = payload[4]; // Track the client's requested bit depth.
                        info!("[VNC] SetPixelFormat, client requested: {} bpp", current_bpp);
                    }
                    Err(_) => return Err(()),
                }
            },
            // Set encoding format.
            2 => {
                // Raw encoding is the only format emitted; drain the offered list.
                let mut header = [0u8; 3];
                match socket.read_exact(&mut header).await {
                    Ok(_) => {
                        let num = u16::from_be_bytes([header[1], header[2]]);
                        for _ in 0..num {
                            let mut enc_data = [0u8; 4];
                            socket.read_exact(&mut enc_data).await.map_err(|_| ())?;
                        }
                    }
                    Err(_) => return Err(()),
                }
            },
            // Client framebuffer update request.
            3 => {
                let mut payload = [0u8; 9];
                if socket.read_exact(&mut payload).await.is_err() {
                    warn!("⚠️ Failed to read FB Request payload");
                    return Err(());
                }
                
                let _inc = payload[0];
                let req_x = u16::from_be_bytes([payload[1], payload[2]]);
                let req_y = u16::from_be_bytes([payload[3], payload[4]]);
                let req_w = u16::from_be_bytes([payload[5], payload[6]]);
                let req_h = u16::from_be_bytes([payload[7], payload[8]]);
                
                let start_x = req_x.min(SCREEN_WIDTH);
                let start_y = req_y.min(SCREEN_HEIGHT);
                let width = req_w.min(SCREEN_WIDTH - start_x);
                let height = req_h.min(SCREEN_HEIGHT - start_y);

                if width == 0 || height == 0 {
                    continue; // Ignore invalid rectangles.
                }

                let mut header = [0u8; 16]; 
                header[0] = 0; 
                header[1] = 0;
                header[2..4].copy_from_slice(&(1u16).to_be_bytes()); // 1 rect
                
                header[4..6].copy_from_slice(&(start_x).to_be_bytes());
                header[6..8].copy_from_slice(&(start_y).to_be_bytes());
                header[8..10].copy_from_slice(&(width).to_be_bytes());
                header[10..12].copy_from_slice(&(height).to_be_bytes());
                header[12..16].copy_from_slice(&(0i32).to_be_bytes()); // Raw
                socket.write_all(&header).await.map_err(|_| ())?;
                
                let bytes_per_pixel = ((current_bpp / 8) as usize).max(1);
                let mut line = [0u8; 800]; 
                
                for y in start_y..(start_y + height) {
                    for x in 0..width {
                        let [r, g, b] = generate_gradient_pixel(start_x + x, start_y + y);
                        let i = (x as usize) * bytes_per_pixel;
                        
                        if bytes_per_pixel == 4 { // 32-bit BGRA.
                            line[i + 0] = b;
                            line[i + 1] = g;
                            line[i + 2] = r;
                            line[i + 3] = 0;
                        } else if bytes_per_pixel == 2 { // 16-bit RGB565 fallback.
                            let rgb565 = ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3);
                            let bytes = rgb565.to_le_bytes();
                            line[i + 0] = bytes[0];
                            line[i + 1] = bytes[1];
                        } else { 
                            line[i] = g; // Grayscale fallback for unusual formats.
                        }
                    }
                    socket.write_all(&line[..(width as usize * bytes_per_pixel)]).await.map_err(|_| ())?;
                }
            },
            // Keyboard event.
            4 => {
                let mut payload = [0u8; 7];
                match socket.read_exact(&mut payload).await {
                    Ok(_) => {
                        let pressed = payload[0] == 1;
                        let key = u32::from_be_bytes([payload[3], payload[4], payload[5], payload[6]]);
                        info!("⌨️  Key Event: Keycode={}, Pressed={}", key, pressed);
                    }
                    Err(_) => {
                        warn!("⚠️  Failed to read KeyEvent payload");
                        return Err(());
                    }
                }
            },
            // Pointer event.
            5 => {
                let mut payload =[0u8; 5];
                match socket.read_exact(&mut payload).await {
                    Ok(_) => {
                        let btn = payload[0];
                        let x = u16::from_be_bytes([payload[1], payload[2]]);
                        let y = u16::from_be_bytes([payload[3], payload[4]]);
                        let click: &str;
                        match btn {
                            0b00000 => click = "None",
                            0b00001 => click = "Left",
                            0b00010 => click = "Middle",
                            0b00100 => click = "Right",
                            0b01000 => click = "Wheel Up",
                            0b10000 => click = "Wheel Down",
                            _ => click = "Unknown",
                        }
                        info!("👉 Mouse Event: X={}, Y={}, Click event={}", x, y, click);
                    }
                    Err(_) => {
                        warn!("⚠️  Failed to read PointerEvent payload");
                        return Err(());
                    }
                }
            },
            // Client clipboard text.
            6 => {
                let mut h = [0u8; 7];
                socket.read_exact(&mut h).await.map_err(|_|())?;
                let len = u32::from_be_bytes([h[3], h[4], h[5], h[6]]);
                for _ in 0..len {
                    let mut b = [0u8; 1];
                    let _ = socket.read_exact(&mut b).await;
                }
            },
            _ => {
                warn!("⚠️  Unknown message type: {} (0x{:02x}), treating as error", msg_type[0], msg_type[0]);
                return Err(());
            }
        }
    }
}
