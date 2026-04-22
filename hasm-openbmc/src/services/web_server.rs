use defmt::*;
use embassy_net::{Stack, tcp::TcpSocket};
use {defmt_rtt as _, panic_probe as _};

use crate::{services::power_control::{is_power_on, set_power_state}, utils::*};


#[embassy_executor::task]
pub async fn http_task(stack: Stack<'static>) {
    let mut tx_buffer = [0u8; 1024];
    let mut rx_buffer = [0u8; 1024];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        info!("HTTP server listening on port {}:80...", stack.config_v4().unwrap().address);
        if let Err(e) = socket.accept(80).await {
            warn!("Accept error: {:?}", e);
            continue;
        }

        handle_http_request(&mut socket).await;
    }
}

pub async fn handle_http_request(socket: &mut embassy_net::tcp::TcpSocket<'_>) {
    let mut buf = [0u8; 1024];
    let mut filled = 0usize;

    // 读取 Header
    'read: loop {
        match socket.read(&mut buf[filled..]).await {
            Ok(0) | Err(_) => break 'read,
            Ok(n) => {
                filled += n;
                if buf[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
                    break 'read;
                }
                if filled >= buf.len() { break 'read; }
            }
        }
    }

    let req_str = core::str::from_utf8(&buf[..filled]).unwrap_or("");
    let mut method = "";
    let mut path = "";
    if let Some(line) = req_str.lines().next() {
        let mut parts = line.split_whitespace();
        method = parts.next().unwrap_or("");
        path = parts.next().unwrap_or("");
    }

    match (method, path) {
        // health check
        ("GET", "/ping") => {
            send_response(socket, 200, b"OK", b"connection up!").await;
        }
        // Redfish root
        ("GET", "/redfish/v1") => {
            send_response(socket, 200, b"OK", ROOT_RESOURCE.as_bytes()).await;
        }
        // Redfish Systems collection
        ("GET", "/redfish/v1/Systems") => {
            send_response(socket, 200, b"OK", SYSTEM_RESOURCE.as_bytes()).await;
        }
        // Redfish Systems, return power state
        ("GET", "/redfish/v1/Systems/1") => {
            send_response(socket, 200, b"OK", dump_system_info("1", is_power_on()).as_bytes()).await;
        }
        // power control
        ("POST", "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset") => {
            if req_str.contains("\"ResetType\":\"On\"") {
                set_power_state(true);
                send_response(socket, 200, b"OK", b"Power On!").await;
            } else if req_str.contains("\"ResetType\":\"ForceOff\"") {
                set_power_state(false);
                send_response(socket, 200, b"OK", b"Force Power Off!").await;
            } else {
                send_response(socket, 404, b"Not Found", b"").await;
            }
        }

        _ => {
            send_response(socket, 404, b"Not Found", b"").await;
        }
    }
    
    // Responses are handled in the match above; avoid duplicate handling here.

    let _ = socket.flush().await;
    socket.close();
}

async fn send_response(
    socket: &mut embassy_net::tcp::TcpSocket<'_>,
    status: u16,
    reason: &[u8],
    body: &[u8],
) {
    use embedded_io_async::Write;
    let mut num_buf = [0u8; 8];
    
    let _ = socket.write_all(b"HTTP/1.1 ").await;
    let _ = socket.write_all(itoa(status, &mut num_buf)).await;
    let _ = socket.write_all(b" ").await;
    let _ = socket.write_all(reason).await;
    let _ = socket.write_all(b"\r\nContent-Type: application/json\r\nContent-Length: ").await;
    let _ = socket.write_all(itoa(body.len() as u16, &mut num_buf)).await;
    let _ = socket.write_all(b"\r\nConnection: close\r\n\r\n").await;
    if !body.is_empty() {
        let _ = socket.write_all(body).await;
    }
}

static ROOT_RESOURCE: &str = r##"{
    "@odata.type": "#ServiceRoot.v1_15_0.ServiceRoot",
    "@odata.id": "/redfish/v1",
    "@odata.context": "/redfish/v1/$metadata#ServiceRoot.ServiceRoot",
    "Id": "RootService",
    "Name": "Root Service",
    "RedfishVersion": "1.13.0",
    "Systems": {
        "@odata.id": "/redfish/v1/Systems"
    },
    "Chassis": {
        "@odata.id": "/redfish/v1/Chassis"
    },
    "Managers": {
        "@odata.id": "/redfish/v1/Managers"
    }
}"##;

static SYSTEM_RESOURCE: &str = r##"{
  "@odata.id": "/redfish/v1/Systems",
  "Members": [
    { "@odata.id": "/redfish/v1/Systems/1" }
  ],
  "Members@odata.count": 1
}"##;