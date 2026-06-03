"""
UART Console Client
Connects to the STM32 UART bridge (169.254.77.2:2323), forwards keyboard input,
and displays serial output in real time.
"""

# Host-side operator console for HASM-OpenBMC.
#
# This tool combines two management paths exposed by the STM32 firmware:
# - the Redfish-like HTTP API on port 80 for ping and power control;
# - the UART bridge on port 2323 for interactive Raspberry Pi serial access.
#
# The menu flow is intentionally synchronous. Operators usually run one command
# at a time while the board is on a bench, so keeping control requests blocking
# makes connection errors visible immediately.
#
# The live console path is more careful:
# - one background thread prints serial output as it arrives;
# - the foreground path forwards keypresses to the TCP socket;
# - Windows gets character-at-a-time input through `msvcrt`;
# - non-Windows terminals fall back to line-at-a-time input.
#
# Telnet negotiation bytes and terminal cursor-position probes are filtered so
# the Raspberry Pi shell does not receive escape noise from local terminal
# emulators.

import socket
import threading
import sys
import os
import json
import argparse
if sys.platform == "win32":
    import msvcrt

# Managed board IP.
HOST = "169.254.77.2"
# Managed board telnet port.
PORT = 2323
# Managed board HTTP service port for power control and status.
HTTP_PORT = 80
# Host-side image server defaults. These must match the firmware constants.
IMG_SERVER_HOST = "169.254.77.1"
IMG_SERVER_PORT = 8000


def parse_args():
    parser = argparse.ArgumentParser(description="HASM-OpenBMC host-side UART and recovery console")
    parser.add_argument("--stm32-host", default=HOST, help="STM32 management/UART IP address")
    parser.add_argument("--uart-port", type=int, default=PORT, help="STM32 UART bridge TCP port")
    parser.add_argument("--http-port", type=int, default=HTTP_PORT, help="STM32 management HTTP port")
    parser.add_argument("--img-host", default=IMG_SERVER_HOST, help="Remote image server IP address")
    parser.add_argument("--img-port", type=int, default=IMG_SERVER_PORT, help="Remote image server HTTP port")
    return parser.parse_args()


def apply_config(args):
    global HOST, PORT, HTTP_PORT, IMG_SERVER_HOST, IMG_SERVER_PORT
    HOST = args.stm32_host
    PORT = args.uart_port
    HTTP_PORT = args.http_port
    IMG_SERVER_HOST = args.img_host
    IMG_SERVER_PORT = args.img_port

MENU = f"""
╔══════════════════════════════╗
║      UART Console Client     ║
║  1. Health check             ║
║  2. Get power state          ║
║  3. Power on                 ║
║  4. Power off                ║
║  5. Force reboot             ║
║  6. Select boot image        ║
║  7. Boot selected image      ║
║  8. Connect UART console     ║
║  9. Exit                     ║
╚══════════════════════════════╝
"""

# ── Telnet IAC filtering ─────────────────────────────────────────────────────
#
# The firmware sends a tiny telnet negotiation sequence to ask clients for
# character mode. Some terminals answer with IAC option bytes and ANSI cursor
# reports. Filtering them here keeps the interactive Pi shell clean.

IAC = 255

import re

# Match CSI cursor-position requests: ESC [ 6 n (Device Status Report).
_DSR_RE = re.compile(rb"\x1b\[6n")
# Match CPR responses: ESC [ rows ; cols R (Cursor Position Report).
_CPR_RE = re.compile(rb"\x1b\[\d+;\d+R")


def _clear_screen():
    # Keep menu redraw behavior native to the current shell.
    os.system("cls" if sys.platform == "win32" else "clear")


def _wait_key_and_back_to_menu():
    # Windows `input()` waits for Enter; `getwch()` gives the intended "any key"
    # behavior for command-prompt users.
    print("\n按任意键返回菜单...")
    if sys.platform == "win32":
        msvcrt.getwch()
    else:
        input()
    _clear_screen()


def _body(resp: str) -> str:
    # The firmware always closes the connection after one response, so splitting
    # at the first blank line is enough for this small client.
    return resp.split("\r\n\r\n", 1)[1].strip() if "\r\n\r\n" in resp else resp.strip()


def _format_size(size: int) -> str:
    units = ("B", "KiB", "MiB", "GiB")
    value = float(size)
    for unit in units:
        if value < 1024 or unit == units[-1]:
            return f"{value:.1f} {unit}" if unit != "B" else f"{size} {unit}"
        value /= 1024
    return f"{size} B"


def _image_server_request(method: str, path: str, payload=None):
    body = "" if payload is None else json.dumps(payload, separators=(",", ":"))
    body_bytes = body.encode("utf-8")
    req = (
        f"{method} {path} HTTP/1.1\r\n"
        f"Host: {IMG_SERVER_HOST}\r\n"
        "Connection: close\r\n"
        "Content-Type: application/json\r\n"
        f"Content-Length: {len(body_bytes)}\r\n"
        "\r\n"
    ).encode("utf-8") + body_bytes

    with socket.create_connection((IMG_SERVER_HOST, IMG_SERVER_PORT), timeout=5) as s:
        s.settimeout(3)
        s.sendall(req)
        data = b""
        while b"\r\n\r\n" not in data:
            chunk = s.recv(4096)
            if not chunk:
                break
            data += chunk

        if b"\r\n\r\n" not in data:
            return 0, data.decode("utf-8", errors="replace")

        head, rest = data.split(b"\r\n\r\n", 1)
        status_line = head.split(b"\r\n", 1)[0].decode("utf-8", errors="replace")
        try:
            status = int(status_line.split()[1])
        except (IndexError, ValueError):
            status = 0

        content_len = 0
        for line in head.split(b"\r\n"):
            if line.lower().startswith(b"content-length:"):
                try:
                    content_len = int(line.split(b":", 1)[1].strip())
                except ValueError:
                    content_len = 0
                break

        body_bytes = rest
        while len(body_bytes) < content_len:
            chunk = s.recv(4096)
            if not chunk:
                break
            body_bytes += chunk
        return status, body_bytes[:content_len].decode("utf-8", errors="replace")


def _image_server_json(method: str, path: str, payload=None):
    status, body = _image_server_request(method, path, payload)
    try:
        data = json.loads(body) if body else {}
    except json.JSONDecodeError:
        data = {"raw": body}
    return status, data


def _check_stm32_health():
    try:
        resp = _http_request("GET", "/ping")
    except OSError as e:
        return False, str(e)
    body = _body(resp)
    if not body:
        return False, "empty response"
    return True, body


def _check_image_server_health():
    try:
        status, body = _image_server_request("GET", "/ping")
    except OSError as e:
        return False, str(e)
    if 200 <= status < 300:
        return True, body
    return False, f"HTTP {status} {body}"


def _show_server_images(data):
    images = data.get("images", [])
    print(f"镜像服务器: http://{IMG_SERVER_HOST}:{IMG_SERVER_PORT}")
    if not images:
        print("服务器没有发现镜像。")
        return
    for image in images:
        marker = "*" if image.get("active") else " "
        print(
            f"{image['index']:2d}. {marker} {image['name']} "
            f"({_format_size(int(image['size']))})"
        )
    if data.get("current"):
        print(f"\n当前启用: {data['current']}")


def get_server_images():
    ok, detail = _check_image_server_health()
    if not ok:
        print(f"[镜像服务器错误] {detail}")
        print(f"拒绝继续：请先确认 image server 正常运行: http://{IMG_SERVER_HOST}:{IMG_SERVER_PORT}")
        return None

    try:
        status, data = _image_server_json("GET", "/images")
    except OSError as e:
        print(f"[镜像服务器错误] {e}")
        print(f"拒绝继续：请先确认 image server 正常运行: http://{IMG_SERVER_HOST}:{IMG_SERVER_PORT}")
        return None
    if status != 200:
        print(data.get("error", data.get("raw", f"HTTP {status}")))
        print("拒绝继续：image server 没有返回可用镜像列表。")
        return None
    if data.get("error"):
        print(data["error"])
        print("拒绝继续：image server 返回异常。")
        return None
    if not data.get("images"):
        print("拒绝继续：image server 当前没有可用镜像。")
        return None
    return data


def select_boot_image():
    data = get_server_images()
    if data is None:
        _wait_key_and_back_to_menu()
        return None

    _show_server_images(data)
    images = data.get("images", [])
    if not images:
        _wait_key_and_back_to_menu()
        return None

    choice = input("\n请选择要启用的镜像编号: ").strip()
    try:
        index = int(choice)
    except ValueError:
        print("无效输入。")
        _wait_key_and_back_to_menu()
        return None

    try:
        status, selected = _image_server_json("POST", "/images/select", {"index": index})
    except OSError as e:
        print(f"[镜像服务器错误] {e}")
        _wait_key_and_back_to_menu()
        return None

    if status != 200:
        print(selected.get("error", selected.get("raw", f"HTTP {status}")))
        _wait_key_and_back_to_menu()
        return None

    print(f"已启用: {selected.get('current')}")
    _wait_key_and_back_to_menu()
    return selected


def _http_request(method: str, path: str, body: str = "") -> str:
    # Build a minimal HTTP/1.1 request compatible with the firmware's tiny parser.
    payload = body.encode("utf-8")
    req = (
        f"{method} {path} HTTP/1.1\r\n"
        f"Host: {HOST}\r\n"
        "Connection: close\r\n"
        "Content-Type: application/json\r\n"
        f"Content-Length: {len(payload)}\r\n"
        "\r\n"
    ).encode("utf-8") + payload

    with socket.create_connection((HOST, HTTP_PORT), timeout=5) as s:
        s.settimeout(3)
        s.sendall(req)
        data = b""
        # First read until the header terminator.
        while b"\r\n\r\n" not in data:
            chunk = s.recv(4096)
            if not chunk:
                break
            data += chunk

        if b"\r\n\r\n" not in data:
            return data.decode("utf-8", errors="replace")

        head, rest = data.split(b"\r\n\r\n", 1)
        content_len = 0
        for line in head.split(b"\r\n"):
            # Honor Content-Length so JSON bodies are complete even when TCP
            # splits headers and body across multiple packets.
            low = line.lower()
            if low.startswith(b"content-length:"):
                try:
                    content_len = int(line.split(b":", 1)[1].strip())
                except ValueError:
                    content_len = 0
                break

        body_bytes = rest
        while len(body_bytes) < content_len:
            chunk = s.recv(4096)
            if not chunk:
                break
            body_bytes += chunk

        full = head + b"\r\n\r\n" + body_bytes[:content_len] if content_len > 0 else head + b"\r\n\r\n" + body_bytes
        return full.decode("utf-8", errors="replace")


def health_check():
    # Check both control-plane dependencies used by this shell.
    print("Health check")
    print(f"- STM32 management API: http://{HOST}:{HTTP_PORT}/ping")
    ok, detail = _check_stm32_health()
    print(f"  {'OK' if ok else 'FAIL'}: {detail}")

    print(f"- Image server: http://{IMG_SERVER_HOST}:{IMG_SERVER_PORT}/ping")
    ok, detail = _check_image_server_health()
    print(f"  {'OK' if ok else 'FAIL'}: {detail}")

    _wait_key_and_back_to_menu()


def get_power_state():
    # Fetch Redfish ComputerSystem and show only the user-visible power state.
    try:
        resp = _http_request("GET", "/redfish/v1/Systems/1")
    except OSError as e:
        print(f"[错误] {e}")
        _wait_key_and_back_to_menu()
        return
    body = _body(resp)
    # Extract the PowerState field.
    import re as _re
    m = _re.search(r'"PowerState"\s*:\s*"([^"]+)"', body)
    print(m.group(1) if m else body)
    _wait_key_and_back_to_menu()


def set_power(reset_type: str, label: str):
    # Firmware matches compact JSON substrings, so separators remove spaces.
    payload = json.dumps({"ResetType": reset_type}, separators=(",", ":"))
    try:
        resp = _http_request("POST", "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset", payload)
    except OSError as e:
        print(f"[错误] {e}")
        _wait_key_and_back_to_menu()
        return False
    print(_body(resp))
    _wait_key_and_back_to_menu()
    return True


def _send_reset(reset_type: str):
    payload = json.dumps({"ResetType": reset_type}, separators=(",", ":"))
    try:
        resp = _http_request("POST", "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset", payload)
    except OSError as e:
        print(f"[错误] {e}")
        return False
    print(_body(resp))
    return True


def force_reboot():
    print("强制重启会先通过 PB3 触发关断，再通过 PB4 触发上电。")
    _send_reset("ForceRestart")
    _wait_key_and_back_to_menu()


def boot_selected_image():
    data = get_server_images()
    if data is None:
        _wait_key_and_back_to_menu()
        return
    print(f"当前启动镜像: {data.get('current')}")
    print("STM32 固件会继续以 USB MSC 设备暴露该远程镜像；正在触发强制重启...")
    _send_reset("ForceRestart")
    _wait_key_and_back_to_menu()

def strip_telnet_negotiation(data: bytes) -> bytes:
    """Strip IAC negotiation sequences and ANSI DSR requests."""
    # Telnet IAC commands can be interleaved with UART bytes. This state machine
    # consumes the common command forms used by the firmware's initial negotiation
    # and leaves normal serial bytes untouched.
    out = bytearray()
    i = 0
    while i < len(data):
        b = data[i]
        if b == IAC and i + 1 < len(data):
            cmd = data[i + 1]
            if cmd == IAC:          # Escaped literal 0xFF.
                out.append(IAC)
                i += 2
            elif cmd in (251, 252, 253, 254):  # WILL/WONT/DO/DONT + option
                i += 3
            elif cmd == 250:        # SB ... SE subnegotiation.
                end = data.find(bytes([IAC, 240]), i + 2)
                i = end + 2 if end != -1 else len(data)
            else:
                i += 2
        else:
            out.append(b)
            i += 1
    # Filter ESC[6n DSR requests so Windows terminals do not echo CPR responses
    # back into stdin.
    result = _DSR_RE.sub(b"", bytes(out))
    return result

# ── Connection session ───────────────────────────────────────────────────────

_stop_event = threading.Event()

def _recv_thread(sock: socket.socket):
    """Background thread: continuously read the socket and print to the terminal."""
    # The receive path is isolated so boot logs keep flowing while the user is
    # thinking or typing commands.
    buf = b""
    while not _stop_event.is_set():
        try:
            chunk = sock.recv(4096)
        except OSError:
            break
        if not chunk:
            print("\r\n[连接已断开]")
            _stop_event.set()
            break
        buf += chunk
        text = strip_telnet_negotiation(buf)
        # Print accumulated output in receive order.
        try:
            decoded = text.decode("utf-8", errors="replace")
        except Exception:
            decoded = text.decode("latin-1", errors="replace")
        sys.stdout.write(decoded)
        sys.stdout.flush()
        buf = b""


def _send_keys_windows(sock: socket.socket):
    """Windows: send each key immediately instead of line buffering."""
    # Windows console input returns special keys as a two-step sequence. Consume
    # those prefixes locally because they are not meaningful to the Pi serial
    # console.
    while not _stop_event.is_set():
        ch = msvcrt.getwch()
        if _stop_event.is_set():
            break

        # Function-key prefix; consume and discard the following key code.
        if ch in ("\x00", "\xe0"):
            _ = msvcrt.getwch()
            continue

        # Suppress terminal CPR echoes and allow a standalone ESC to leave the
        # session.
        if ch == "\x1b":
            seq = [ch]
            # Try to read a short trailing escape sequence. If no key follows,
            # seq_s remains just "\x1b".
            for _ in range(16):
                if msvcrt.kbhit():
                    seq.append(msvcrt.getwch())
                    if seq[-1] == "R":
                        break
                else:
                    break
            seq_s = "".join(seq)
            # Ignore CPR responses.
            if re.fullmatch(r"\x1b\[\d+;\d+R", seq_s):
                continue
            # A standalone ESC leaves the session without forwarding it.
            if seq_s == "\x1b":
                _stop_event.set()
                break
            data = seq_s.encode("utf-8", errors="ignore")
        elif ch in ("\r", "\n"):
            data = b"\n"
        elif ch == "\x08":
            # Backspace -> DEL; the firmware bridge normalizes it to BS.
            data = b"\x7f"
        else:
            data = ch.encode("utf-8", errors="ignore")

        if not data:
            continue
        try:
            sock.sendall(data)
        except OSError:
            _stop_event.set()
            break


def connect():
    # Establish one interactive UART session. Leaving the session returns to the
    # management menu instead of exiting the whole tool.
    _clear_screen()
    ok, detail = _check_stm32_health()
    if not ok:
        print(f"拒绝连接 UART console：STM32 management API health check 失败: {detail}")
        print(f"请先确认 STM32 在线: http://{HOST}:{HTTP_PORT}/ping\n")
        return

    print(f"正在连接 {HOST}:{PORT} ...\n")
    try:
        sock = socket.create_connection((HOST, PORT), timeout=5)
    except OSError as e:
        print(f"连接失败: {e}\n")
        return

    sock.settimeout(None)
    _stop_event.clear()

    recv_t = threading.Thread(target=_recv_thread, args=(sock,), daemon=True)
    recv_t.start()

    print("按 Ctrl+C 结束会话。\n")

    try:
        if sys.platform == "win32":
            _send_keys_windows(sock)
        else:
            # Non-Windows fallback: send one line at a time.
            while not _stop_event.is_set():
                try:
                    line = input()
                except EOFError:
                    break
                if _stop_event.is_set():
                    break
                try:
                    sock.sendall((line + "\n").encode("utf-8"))
                except OSError as e:
                    print(f"\r\n[发送失败: {e}]")
                    break
    except KeyboardInterrupt:
        print("\r\n[Ctrl+C 中断]")
    finally:
        _stop_event.set()
        sock.close()
        recv_t.join(timeout=1)
        _clear_screen()

# ── Main menu ────────────────────────────────────────────────────────────────

def main():
    apply_config(parse_args())

    # Ensure the Windows command prompt can display UTF-8.
    if sys.platform == "win32":
        os.system("chcp 65001 >nul 2>&1")

    # The menu stays in the foreground; long-running interaction happens only
    # inside `connect`, which returns here after the socket closes.
    while True:
        print(f"STM32: {HOST}:{HTTP_PORT} / UART {PORT} | Image server: {IMG_SERVER_HOST}:{IMG_SERVER_PORT}")
        print(MENU)
        choice = input("请选择: ").strip()
        if choice == "1":
            health_check()
        elif choice == "2":
            get_power_state()
        elif choice == "3":
            set_power("On", "Power on")
        elif choice == "4":
            set_power("ForceOff", "Power off")
        elif choice == "5":
            force_reboot()
        elif choice == "6":
            select_boot_image()
        elif choice == "7":
            boot_selected_image()
        elif choice == "8":
            connect()
        elif choice == "9":
            print("再见！")
            sys.exit(0)
        else:
            print("无效输入，请输入 1-9。\n")


if __name__ == "__main__":
    main()
