//! Minimal HTTP/Redfish-like management service.
//!
//! The service exposes just enough Redfish shape for host tools or scripts to
//! inspect and toggle the controlled Raspberry Pi power state:
//! - `/ping` for a simple connectivity check;
//! - `/redfish/v1` and `/redfish/v1/Systems` for service discovery;
//! - `/redfish/v1/Systems/1` for current power state;
//! - `ComputerSystem.Reset` POST for power on, force-off, and force-restart requests.
//!
//! Request parsing is intentionally tiny. The board is used on an isolated lab
//! network, and the Python console tool sends fixed requests. Avoiding a general
//! HTTP parser keeps RAM usage predictable in `no_std`.

use defmt::*;
use embassy_net::{Stack, tcp::TcpSocket};
use {defmt_rtt as _, panic_probe as _};

use crate::{services::power_control::{is_power_on, request_power_action, PowerAction}, utils::*};

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

/// Read one HTTP request, dispatch the supported management endpoint, and close
/// the connection.
pub async fn handle_http_request(socket: &mut embassy_net::tcp::TcpSocket<'_>) {
    let mut buf = [0u8; 1024];
    let mut filled = 0usize;

    // Read headers. Stop at CRLF CRLF because no supported endpoint needs to
    // stream a large request body.
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
    // Only the request line is needed for routing. Headers are ignored except
    // that the reset handler later searches the small body for `ResetType`.
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
            // Generate this response dynamically so the PowerState field follows
            // the atomic flag updated by the power-control task.
            send_response(socket, 200, b"OK", dump_system_info("1", is_power_on()).as_bytes()).await;
        }
        // power control
        ("POST", "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset") => {
            // The console helper sends compact JSON with no extra whitespace.
            // This substring check is enough for the controlled tooling and
            // avoids pulling in a JSON parser for firmware-side management.
            if req_str.contains("\"ResetType\":\"On\"") {
                request_power_action(PowerAction::On);
                send_response(socket, 200, b"OK", b"Power On!").await;
            } else if req_str.contains("\"ResetType\":\"ForceOff\"") {
                request_power_action(PowerAction::ForceOff);
                send_response(socket, 200, b"OK", b"Force Power Off!").await;
            } else if req_str.contains("\"ResetType\":\"ForceRestart\"") {
                request_power_action(PowerAction::ForceRestart);
                send_response(socket, 200, b"OK", b"Force Restart!").await;
            } else {
                send_response(socket, 404, b"Not Found", b"").await;
            }
        }

        _ => {
            // Unknown routes return an empty 404. Keeping the body empty makes it
            // cheap for scripts to treat any non-supported endpoint as a simple
            // miss without parsing an error document.
            send_response(socket, 404, b"Not Found", b"").await;
        }
    }
    
    // Responses are handled in the match above; avoid duplicate handling here.

    let _ = socket.flush().await;
    socket.close();
}

/// Write a small HTTP/1.1 response without heap allocation.
async fn send_response(
    socket: &mut embassy_net::tcp::TcpSocket<'_>,
    status: u16,
    reason: &[u8],
    body: &[u8],
) {
    use embedded_io_async::Write;
    let mut num_buf = [0u8; 8];
    
    // Compose the response from fixed byte slices and stack-formatted numbers.
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

// Static Redfish discovery resources. System state itself is generated in
// `utils::dump_system_info` so the power flag can be reflected at request time.
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
