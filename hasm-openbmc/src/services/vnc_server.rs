//! Experimental VNC/RFB service.
//!
//! This task implements a very small subset of RFB 3.8 for diagnostics: it
//! accepts a no-auth client, advertises a fixed 200x200 framebuffer, and returns
//! generated gradient pixels for framebuffer update requests.
//!
//! It is useful as a network/display protocol exercise and a visual heartbeat,
//! but it is not wired into `main.rs` by default. Production power/boot control
//! uses the UART, Redfish, and USB MSC services instead.
//!
//! The server handles client pixel-format requests for 32-bit and 16-bit output
//! so common VNC viewers can connect without custom settings.

use embassy_net::{Stack, tcp::TcpSocket};
use embedded_io_async::{Read, Write};
use defmt::{info, warn};
use {defmt_rtt as _, panic_probe as _};

const SCREEN_WIDTH: u16 = 200;
const SCREEN_HEIGHT: u16 = 200; 

#[embassy_executor::task]
pub async fn vnc_task(stack: Stack<'static>) {
    let mut rx_buffer = [0u8; 4096];
    let mut tx_buffer = [0u8; 4096];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        info!("VNC server listening on port {}:·5900...", stack.config_v4().unwrap().address);
        if let Err(e) = socket.accept(5900).await {
            warn!("Accept error: {:?}", e);
            continue;
        }

        handle_vnc_session(&mut socket).await;
    }
}

/// Handle one VNC/RFB client session.
pub async fn handle_vnc_session(socket: &mut TcpSocket<'_>) {
    // RFB version negotiation.
    let _ = socket.write_all(b"RFB 003.008\n").await;
    let mut client_version = [0u8; 12];
    let _ = socket.read_exact(&mut client_version).await;

    // Security negotiation: advertise and accept "None" authentication.
    let _ = socket.write_all(&[1, 1]).await;
    let mut sec_type =[0u8; 1];
    let _ = socket.read_exact(&mut sec_type).await;
    if sec_type[0] != 1 { return; }
    let _ = socket.write_all(&[0, 0, 0, 0]).await;

    let mut client_init = [0u8; 1];
    let _ = socket.read_exact(&mut client_init).await;

    // ServerInit message with a fixed framebuffer and the server name.
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
    
    let _ = socket.write_all(&server_init).await;
    info!("✓ Server Init Sent. Entering Event Loop!");

    let mut current_bpp = 32u8;

    loop {
        let mut msg_type = [0u8; 1];
        if socket.read_exact(&mut msg_type).await.is_err() {
            warn!("⚠️  Failed to read message type, connection lost");
            return;
        }

        match msg_type[0] {
            // Set pixel format.
            0 => {
                // SetPixelFormat has 3 bytes of padding followed by the 16-byte
                // PixelFormat structure. Only bits-per-pixel matters for the
                // synthetic raw framebuffer below.
                let mut payload = [0u8; 19];
                match socket.read_exact(&mut payload).await {
                    Ok(_) => {
                        current_bpp = payload[4]; // Track the bit depth requested by the client.
                        info!("[VNC] SetPixelFormat, client requested: {} bpp", current_bpp);
                    }
                    Err(_) => return,
                }
            },
            // Set encoding format.
            2 => {
                // The current framebuffer is raw-only; read and discard the
                // client's encoding list so the stream remains aligned.
                let mut header = [0u8; 3];
                match socket.read_exact(&mut header).await {
                    Ok(_) => {
                        let num = u16::from_be_bytes([header[1], header[2]]);
                        for _ in 0..num {
                            let mut enc_data = [0u8; 4];
                            let _ = socket.read_exact(&mut enc_data).await;
                        }
                    }
                    Err(_) => return,
                }
            },
            // Client framebuffer update request.
            3 => {
                // FramebufferUpdateRequest asks for a rectangle. Clamp it to the
                // fixed framebuffer so malformed clients cannot overflow `line`.
                let mut payload = [0u8; 9];
                if socket.read_exact(&mut payload).await.is_err() {
                    warn!("⚠️ Failed to read FB Request payload");
                    return;
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
                let _ = socket.write_all(&header).await;
                
                let bytes_per_pixel = ((current_bpp / 8) as usize).max(1);
                // Worst case is 200 pixels * 4 bytes, matching the 800-byte
                // line buffer. Requests are clamped to screen bounds above.
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
                            line[i] = g; 
                        }
                    }
                    let _ = socket.write_all(&line[..(width as usize * bytes_per_pixel)]).await;
                }
            },
            // Keyboard event.
            4 => {
                // Keyboard events are logged only; this diagnostic VNC server is
                // not connected to the Raspberry Pi input path.
                let mut payload = [0u8; 7];
                match socket.read_exact(&mut payload).await {
                    Ok(_) => {
                        let pressed = payload[0] == 1;
                        let key = u32::from_be_bytes([payload[3], payload[4], payload[5], payload[6]]);
                        info!("⌨️  Key Event: Keycode={}, Pressed={}", key, pressed);
                    }
                    Err(_) => {
                        warn!("⚠️  Failed to read KeyEvent payload");
                        return;
                    }
                }
            },
            // Pointer event.
            5 => {
                // Pointer events are also diagnostic-only. Decoding button bits
                // makes viewer behavior visible in defmt logs.
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
                        return;
                    }
                }
            },
            // Client clipboard text.
            6 => {
                // ClientCutText payload is not used; drain it so later messages
                // still start at the correct byte boundary.
                let mut h = [0u8; 7];
                let _ = socket.read_exact(&mut h).await;
                let len = u32::from_be_bytes([h[3], h[4], h[5], h[6]]);
                for _ in 0..len {
                    let mut b = [0u8; 1];
                    let _ = socket.read_exact(&mut b).await;
                }
            },
            _ => {
                warn!("⚠️  Unknown message type: {} (0x{:02x}), treating as error", msg_type[0], msg_type[0]);
                return;
            }
        }
    }
}

/// Produce a deterministic RGB gradient for the synthetic framebuffer.
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
