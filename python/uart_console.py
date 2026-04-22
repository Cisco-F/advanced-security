"""
UART Console Client
连接至 STM32 UART bridge (192.168.1.177:2323)，转发键盘输入并实时显示串口输出。
"""

import socket
import threading
import sys
import os
import json
if sys.platform == "win32":
    import msvcrt

HOST = "192.168.1.177"
PORT = 2323
HTTP_PORT = 80

MENU = """
╔══════════════════════════════╗
║      UART Console Client     ║
║  1. Get power state          ║
║  2. Power on                 ║
║  3. Power off                ║
║  4. Connect  (192.168.1.177) ║
║  5. Exit                     ║
╚══════════════════════════════╝
"""

# ── Telnet IAC 协议字节过滤 ──────────────────────────────────────────────────

IAC = 255

import re

# 匹配 CSI 光标位置请求 ESC [ 6 n（Device Status Report）
_DSR_RE = re.compile(rb"\x1b\[6n")
# 匹配 CPR 响应 ESC [ rows ; cols R（Cursor Position Report）
_CPR_RE = re.compile(rb"\x1b\[\d+;\d+R")


def _clear_screen():
    os.system("cls" if sys.platform == "win32" else "clear")


def _wait_key_and_back_to_menu():
    print("\n按任意键返回菜单...")
    if sys.platform == "win32":
        msvcrt.getwch()
    else:
        input()
    _clear_screen()


def _print_http_response(resp: str):
    status_line = resp.split("\r\n", 1)[0] if resp else ""
    body = resp.split("\r\n\r\n", 1)[1] if "\r\n\r\n" in resp else ""
    print("\n===== HTTP RESPONSE =====")
    print(status_line if status_line else "(empty status)")
    print("----- BODY -----")
    print(body if body else "(empty body)")
    print("=========================\n")


def _http_request(method: str, path: str, body: str = "") -> str:
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
        # 先读到 header 结束
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


def get_power_state():
    try:
        resp = _http_request("GET", "/redfish/v1/Systems/1")
    except OSError as e:
        print(f"\n[HTTP 请求失败] {e}\n")
        _wait_key_and_back_to_menu()
        return
    _print_http_response(resp)
    _wait_key_and_back_to_menu()


def set_power(reset_type: str, label: str):
    body = json.dumps({"ResetType": reset_type}, separators=(",", ":"))
    try:
        resp = _http_request("POST", "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset", body)
    except OSError as e:
        print(f"\n[HTTP 请求失败] {e}\n")
        _wait_key_and_back_to_menu()
        return
    print(f"\n[{label}] 请求完成")
    _print_http_response(resp)
    _wait_key_and_back_to_menu()

def strip_telnet_negotiation(data: bytes) -> bytes:
    """去掉 IAC 协商序列和 ANSI DSR 请求，返回纯数据。"""
    out = bytearray()
    i = 0
    while i < len(data):
        b = data[i]
        if b == IAC and i + 1 < len(data):
            cmd = data[i + 1]
            if cmd == IAC:          # 转义的 0xFF 本身
                out.append(IAC)
                i += 2
            elif cmd in (251, 252, 253, 254):  # WILL/WONT/DO/DONT + option
                i += 3
            elif cmd == 250:        # SB ... SE 子协商
                end = data.find(bytes([IAC, 240]), i + 2)
                i = end + 2 if end != -1 else len(data)
            else:
                i += 2
        else:
            out.append(b)
            i += 1
    # 过滤 ESC[6n（DSR 请求），防止 Windows 终端回注 CPR 响应到 stdin
    result = _DSR_RE.sub(b"", bytes(out))
    return result

# ── 连接会话 ─────────────────────────────────────────────────────────────────

_stop_event = threading.Event()

def _recv_thread(sock: socket.socket):
    """后台线程：持续读取 socket 并打印到终端。"""
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
        # 只输出完整行或积累的内容，避免乱序
        try:
            decoded = text.decode("utf-8", errors="replace")
        except Exception:
            decoded = text.decode("latin-1", errors="replace")
        sys.stdout.write(decoded)
        sys.stdout.flush()
        buf = b""


def _send_keys_windows(sock: socket.socket):
    """Windows: 按键即发，避免整行缓冲。"""
    while not _stop_event.is_set():
        ch = msvcrt.getwch()
        if _stop_event.is_set():
            break

        # 功能键前缀，丢弃后续键码
        if ch in ("\x00", "\xe0"):
            _ = msvcrt.getwch()
            continue

        # Ctrl+C: 中断会话
        if ch == "\x03":
            _stop_event.set()
            break

        # 屏蔽终端回注 CPR（ESC [ rows ; cols R）
        if ch == "\x1b":
            seq = [ch]
            # 尝试读取形如 ESC [ 1 ; 14 R 的短序列
            for _ in range(16):
                if msvcrt.kbhit():
                    seq.append(msvcrt.getwch())
                    if seq[-1] == "R":
                        break
                else:
                    break
            seq_s = "".join(seq)
            if re.fullmatch(r"\x1b\[\d+;\d+R", seq_s):
                continue
            data = seq_s.encode("utf-8", errors="ignore")
        elif ch in ("\r", "\n"):
            data = b"\n"
        elif ch == "\x08":
            # Backspace -> DEL，让固件桥接转换为 BS
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
    print(f"\n正在连接 {HOST}:{PORT} ...\n")
    try:
        sock = socket.create_connection((HOST, PORT), timeout=5)
    except OSError as e:
        print(f"连接失败: {e}\n")
        return

    sock.settimeout(None)
    _stop_event.clear()

    recv_t = threading.Thread(target=_recv_thread, args=(sock,), daemon=True)
    recv_t.start()

    print("已连接。现在是逐字符发送模式（按键立即生效）。按 Ctrl+C 结束会话。\n")

    try:
        if sys.platform == "win32":
            _send_keys_windows(sock)
        else:
            # 非 Windows 的兼容回退：逐行发送
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
        print("\n会话结束。\n")

# ── 主菜单 ───────────────────────────────────────────────────────────────────

def main():
    # Windows 下保证 cmd 黑窗口可以显示 UTF-8
    if sys.platform == "win32":
        os.system("chcp 65001 >nul 2>&1")

    while True:
        print(MENU)
        choice = input("请选择: ").strip()
        if choice == "1":
            get_power_state()
        elif choice == "2":
            set_power("On", "Power on")
        elif choice == "3":
            set_power("ForceOff", "Power off")
        elif choice == "4":
            connect()
        elif choice == "5":
            print("再见！")
            sys.exit(0)
        else:
            print("无效输入，请输入 1-5。\n")


if __name__ == "__main__":
    main()
