//! TCP-to-UART console bridge.
//!
//! The host connects to `TELNET_PORT` and receives the Raspberry Pi serial boot
//! log through USART1. Bytes typed by the host are forwarded back to the Pi so the
//! same session can be used as an interactive recovery console.
//!
//! Telnet is used only as a convenient TCP terminal protocol. The firmware does
//! not implement full option negotiation; it requests character mode and then
//! treats the stream mostly as raw bytes.
//!
//! The bridge runs a bidirectional `select`: whichever direction exits first
//! ends the session and lets the outer loop accept a new client.

use defmt::*;
use embassy_futures::select::{select, Either};
use embassy_net::{Stack, tcp::TcpSocket};
use embassy_stm32::usart::BufferedUart;
use embedded_io_async::{Read, Write};
use crate::consts::TELNET_PORT;

use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::task]
pub async fn console_task(mut uart: BufferedUart<'static>, stack: Stack<'static>) {
    info!("UART console listening on port {}", TELNET_PORT);

    let mut socket_rx_buffer = [0u8; 1024];
    let mut socket_tx_buffer = [0u8; 1024];

    loop {
        let mut socket = TcpSocket::new(stack, &mut socket_rx_buffer, &mut socket_tx_buffer);
        socket.set_keep_alive(Some(embassy_time::Duration::from_secs(10)));

        if let Err(e) = socket.accept(TELNET_PORT).await {
            warn!("accept error: {:?}", e);
            continue;
        }

        info!("console client connected: {:?}", socket.remote_endpoint());

        if let Err(e) = socket
            .write_all(b"STM32 UART bridge connected. Waiting for Raspberry Pi boot log...\r\n")
            .await
        {
            warn!("banner write error: {:?}", e);
            socket.abort();
            let _ = socket.flush().await;
            continue;
        }

        // Basic telnet negotiation: request character mode
        let _ = socket.write_all(&[255u8, 253u8, 3u8, 255u8, 251u8, 1u8]).await;

        match bridge_session(&mut uart, &mut socket).await {
            Ok(()) => info!("console client closed"),
            Err(()) => warn!("console session ended"),
        }

        socket.abort();
        let _ = socket.flush().await;
    }
}

/// Bridge one accepted TCP session to the buffered UART.
async fn bridge_session(uart: &mut BufferedUart<'_>, socket: &mut TcpSocket<'_>) -> Result<(), ()> {
    let (mut reader, mut writer) = socket.split();
    let (mut tx, mut rx) = uart.split_ref();

    let uart_to_tcp = async {
        let mut buf = [0u8; 128];
        loop {
            // Forward Pi output as soon as bytes are available so boot logs feel
            // live to the remote operator.
            let n = match rx.read(&mut buf).await {
                Ok(n) => n,
                Err(_) => return Err(()),
            };
            if n == 0 {
                continue;
            }
            if writer.write_all(&buf[..n]).await.is_err() {
                return Err(());
            }
        }
    };

    let tcp_to_uart = async {
        let mut inbuf = [0u8; 256];
        let mut out = [0u8; 512];
        loop {
            // Terminal programs differ on Enter and Backspace encoding. Normalize
            // here so the Pi sees the conventional serial-console bytes.
            let n = match reader.read(&mut inbuf).await {
                Ok(n) => n,
                Err(_) => return Err(()),
            };
            if n == 0 {
                return Err(());
            }

            // DEL(0x7f) -> BS(0x08), normalize CR/CRLF -> LF
            let mut wi = 0usize;
            let mut i = 0usize;
            while i < n {
                let b = inbuf[i];
                if b == 0x7f {
                    out[wi] = 0x08;
                    wi += 1;
                    i += 1;
                } else if b == b'\r' {
                    out[wi] = b'\n';
                    wi += 1;
                    if i + 1 < n && inbuf[i + 1] == b'\n' {
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else {
                    out[wi] = b;
                    wi += 1;
                    i += 1;
                }
            }

            if wi > 0 && tx.write_all(&out[..wi]).await.is_err() {
                return Err(());
            }
        }
    };

    match select(uart_to_tcp, tcp_to_uart).await {
        Either::First(r) | Either::Second(r) => r,
    }
}
