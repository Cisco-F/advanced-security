#!/usr/bin/env python3
#
# Host-side disk-image range server for HASM-OpenBMC.
#
# The STM32 firmware exposes a USB mass-storage device to the Raspberry Pi, but
# it does not keep the whole disk image in MCU flash or RAM. Instead, the
# firmware translates SCSI sector reads into HTTP Range requests against this
# server. This script is therefore the backing store for remote USB boot.
#
# Design notes:
# - `/image` accepts normal GET requests and single `Range: bytes=start-end`
#   requests.
# - Ranges are inclusive, matching HTTP semantics and the firmware's sector math.
# - Optional preload mode keeps the image in process memory for faster repeated
#   boot tests.
# - The server is intended for an isolated lab network, not the public Internet.
#
# Operational expectations:
# - Run this script on the host PC before powering the Raspberry Pi.
# - Keep the host IP in sync with `hasm-openbmc/src/consts.rs`.
# - Use a raw disk image whose partition table and boot files are ready for the
#   Pi boot mode being tested.
# - Do not expose the server outside the bench network; there is no auth layer.
#
# Error handling favors firmware visibility over HTTP sophistication. Bad ranges
# return simple status codes, while valid ranges stream exactly the requested
# bytes so the STM32 can fill SCSI READ(10) responses without guessing.
"""Simple HTTP server to serve a host-side boot image for STM32 testing.

Features:
- Serves full image at `/image` with support for `Range` requests.
- Serves fixed-size 512-byte blocks at `/block/{lba}` (optional `?count=N`).
- Lists and selects images through `/images` and `/images/select`.
- Optional `--preload` to load the whole image into RAM for lower latency.

Usage:
    python python/remote_image_server.py
    python python/remote_image_server.py --host 169.254.77.1 --port 8000 --img img/raspi_recover.img --preload

Requires: Python 3.8+ and aiohttp: `pip install aiohttp`
"""
import argparse
import asyncio
import json
from pathlib import Path
from aiohttp import web

ROOT_DIR = Path(__file__).resolve().parents[1]
IMAGE_DIR = ROOT_DIR / "img"
DEFAULT_IMAGE = IMAGE_DIR / "raspi_recover.img"
IMAGE_EXTS = {".img", ".iso", ".raw"}


def parse_args():
    # Defaults match the firmware constants and README quick-start instructions.
    p = argparse.ArgumentParser()
    p.add_argument("--host", default="169.254.77.1")
    p.add_argument("--port", type=int, default=8000)
    p.add_argument("--img-dir", default=str(IMAGE_DIR), help="Directory containing boot images")
    p.add_argument("--img", default=None, help="Initial image path or name inside --img-dir")
    p.add_argument("--preload", action="store_true", help="Preload image into memory")
    return p.parse_args()


def find_images(img_dir):
    img_dir = Path(img_dir)
    if not img_dir.exists():
        raise FileNotFoundError(f"image directory not found: {img_dir}")
    return sorted(
        p.resolve() for p in img_dir.iterdir()
        if p.is_file() and p.suffix.lower() in IMAGE_EXTS
    )


def resolve_initial_image(img_arg, images):
    if not images:
        raise SystemExit("no .img/.iso/.raw images found")

    if img_arg:
        requested = Path(img_arg)
        if not requested.is_absolute():
            requested = (ROOT_DIR / requested).resolve()
        if requested in images:
            return requested
        for image in images:
            if image.name == img_arg:
                return image
        raise SystemExit(f"initial image not found in image directory: {img_arg}")

    default = DEFAULT_IMAGE.resolve()
    if default in images:
        return default
    print(f"[server] auto-selected image: {images[0]}")
    return images[0]


def set_current_image(app, img_path):
    img_path = Path(img_path).resolve()
    if not img_path.exists():
        raise FileNotFoundError(img_path)

    app['img_path'] = img_path
    app['img_size'] = img_path.stat().st_size
    if app['preload']:
        print(f"[server] preloading image: {img_path}")
        with open(img_path, 'rb') as f:
            app['img_mem'] = f.read()
        print("[server] preload done, size", len(app['img_mem']))
    else:
        app.pop('img_mem', None)


def image_payload(app):
    images = app['images']
    current = app['img_path']
    current_index = next((i for i, image in enumerate(images, start=1) if image == current), None)
    return {
        "current_index": current_index,
        "current": current.name,
        "current_path": str(current),
        "images": [
            {
                "index": idx,
                "name": image.name,
                "path": str(image),
                "size": image.stat().st_size,
                "active": image == current,
            }
            for idx, image in enumerate(images, start=1)
        ],
    }


def refresh_images(app):
    images = find_images(app['img_dir'])
    if not images:
        raise FileNotFoundError("no .img/.iso/.raw images found")
    app['images'] = images
    if app.get('img_path') not in images:
        set_current_image(app, resolve_initial_image(None, images))


async def handle_ping(request):
    # Lightweight endpoint for checking that the host server is reachable.
    return web.Response(text="OK")


async def handle_images(request):
    try:
        refresh_images(request.app)
    except FileNotFoundError as e:
        return web.json_response({"error": str(e), "images": []}, status=404)
    return web.json_response(image_payload(request.app))


async def handle_select_image(request):
    app = request.app
    try:
        refresh_images(app)
    except FileNotFoundError as e:
        return web.json_response({"error": str(e), "images": []}, status=404)

    try:
        body = await request.text()
        payload = json.loads(body or "{}")
    except json.JSONDecodeError:
        return web.json_response({"error": "bad json"}, status=400)

    images = app['images']
    selected = None
    if "index" in payload:
        try:
            index = int(payload["index"])
        except (TypeError, ValueError):
            return web.json_response({"error": "bad index"}, status=400)
        if index < 1 or index > len(images):
            return web.json_response({"error": "index out of range"}, status=400)
        selected = images[index - 1]
    elif "name" in payload:
        selected = next((image for image in images if image.name == payload["name"]), None)
        if selected is None:
            return web.json_response({"error": "name not found"}, status=404)
    else:
        return web.json_response({"error": "index or name required"}, status=400)

    set_current_image(app, selected)
    print(f"[server] selected image: {selected}")
    return web.json_response(image_payload(app))


def http_range_to_slice(range_header, file_size):
    # supports single range "bytes=start-end"
    # The firmware sends inclusive HTTP ranges derived from LBA * 512. This
    # helper also accepts suffix ranges for manual testing with curl.
    if not range_header or not range_header.startswith("bytes="):
        return None
    spec = range_header[len("bytes="):]
    if '-' not in spec:
        return None
    start_str, end_str = spec.split('-', 1)
    try:
        if start_str == '':
            # suffix: bytes=-N -> last N bytes
            n = int(end_str)
            start = max(0, file_size - n)
            end = file_size - 1
        elif end_str == '':
            start = int(start_str)
            end = file_size - 1
        else:
            start = int(start_str)
            end = int(end_str)
    except ValueError:
        return None
    if start < 0 or end >= file_size or start > end:
        return None
    return start, end


async def handle_image(request):
    # Main firmware data path: SCSI sector reads arrive here as HTTP byte ranges.
    app = request.app
    try:
        refresh_images(app)
    except FileNotFoundError as e:
        return web.Response(status=404, text=str(e))
    img_path = app['img_path']
    size = app['img_size']
    data = app.get('img_mem')

    # log request
    print(f"[server] {request.remote} GET /image Range={request.headers.get('Range')}")

    range_hdr = request.headers.get('Range')
    r = http_range_to_slice(range_hdr, size)
    if r is None:
        # full response
        # A full response is useful for manual download tests. Firmware reads
        # normally include a Range header and take the 206 branch below.
        # Returning Accept-Ranges in both branches lets simple tools detect that
        # the same endpoint can serve partial requests.
        headers = {
            'Content-Type': 'application/octet-stream',
            'Content-Length': str(size),
            'Accept-Ranges': 'bytes',
        }
        if data is not None:
            return web.Response(body=data, headers=headers)
        else:
            return web.FileResponse(img_path, headers=headers)
    else:
        start, end = r
        length = end - start + 1
        # Content-Range is required for a proper 206 response and is also useful
        # when debugging firmware logs against server logs.
        headers = {
            'Content-Type': 'application/octet-stream',
            'Content-Length': str(length),
            'Content-Range': f'bytes {start}-{end}/{size}',
            'Accept-Ranges': 'bytes',
        }
        if data is not None:
            # Preload mode serves directly from memory, reducing host disk jitter
            # during repeated boot tests.
            return web.Response(status=206, body=data[start:end+1], headers=headers)
        else:
            # stream from file
            # File mode keeps memory use bounded for larger images and streams in
            # chunks so the aiohttp loop can keep making progress.
            resp = web.StreamResponse(status=206, headers=headers)
            await resp.prepare(request)
            with open(img_path, 'rb') as f:
                f.seek(start)
                remaining = length
                chunk_size = 64 * 1024
                while remaining > 0:
                    to_read = min(chunk_size, remaining)
                    chunk = f.read(to_read)
                    if not chunk:
                        break
                    await resp.write(chunk)
                    remaining -= len(chunk)
            await resp.write_eof()
            return resp


async def handle_block(request):
    # Optional debugging endpoint for fetching block-aligned slices by LBA. The
    # current firmware uses `/image`, but `/block/{lba}` is handy when comparing
    # sector contents from a browser or curl.
    app = request.app
    try:
        refresh_images(app)
    except FileNotFoundError as e:
        return web.Response(status=404, text=str(e))
    img_path = app['img_path']
    size = app['img_size']
    data = app.get('img_mem')

    # log request
    print(f"[server] {request.remote} GET /block/{request.match_info.get('lba')} count={request.query.get('count')}")

    lba_s = request.match_info.get('lba')
    try:
        lba = int(lba_s)
    except Exception:
        return web.Response(status=400, text='bad lba')
    count = int(request.query.get('count', '1'))
    if count < 1:
        return web.Response(status=400, text='bad count')
    # Convert logical blocks to byte offsets using the same 512-byte sector size
    # advertised by the firmware's USB MSC layer.
    offset = lba * 512
    length = count * 512
    if offset < 0 or offset + length > size:
        return web.Response(status=416, text='Requested Range Not Satisfiable')

    headers = {
        'Content-Type': 'application/octet-stream',
        'Content-Length': str(length),
    }
    if data is not None:
        return web.Response(body=data[offset:offset+length], headers=headers)
    else:
        with open(img_path, 'rb') as f:
            f.seek(offset)
            body = f.read(length)
        return web.Response(body=body, headers=headers)


async def init_app(img_dir, initial_img=None, preload=False):
    # Store image metadata in the aiohttp app so each request handler avoids
    # repeating stat calls and path validation.
    app = web.Application()
    app['img_dir'] = Path(img_dir)
    app['preload'] = preload
    try:
        app['images'] = find_images(app['img_dir'])
    except FileNotFoundError as e:
        raise SystemExit(str(e)) from e
    set_current_image(app, resolve_initial_image(initial_img, app['images']))

    app.router.add_get('/ping', handle_ping)
    app.router.add_get('/images', handle_images)
    app.router.add_post('/images/select', handle_select_image)
    # `/image` is the production firmware path; `/block` is a human-friendly
    # diagnostic path.
    app.router.add_get('/image', handle_image)
    app.router.add_get('/block/{lba}', handle_block)

    return app


def main():
    args = parse_args()
    # aiohttp owns the event loop after `web.run_app`; initialization is kept in
    # an async helper so future startup checks can share the same loop style.
    app = asyncio.run(init_app(args.img_dir, initial_img=args.img, preload=args.preload))
    print(f"Serving image directory {args.img_dir} on http://{args.host}:{args.port} (preload={args.preload})")
    print(f"Current image: {app['img_path']}")
    web.run_app(app, host=args.host, port=args.port)


if __name__ == '__main__':
    main()
