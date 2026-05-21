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
"""Simple HTTP server to serve img/raspi_recover.img for STM32 testing.

Features:
- Serves full image at `/image` with support for `Range` requests.
- Serves fixed-size 512-byte blocks at `/block/{lba}` (optional `?count=N`).
- Optional `--preload` to load the whole image into RAM for lower latency.

Usage:
    python tools/remote_image_server.py --host 192.168.1.77 --port 8000 --img img/raspi_recover.img --preload

Requires: Python 3.8+ and aiohttp: `pip install aiohttp`
"""
import argparse
import asyncio
import os
from aiohttp import web


def parse_args():
    # Defaults match the firmware constants and README quick-start instructions.
    p = argparse.ArgumentParser()
    p.add_argument("--host", default="192.168.1.77")
    p.add_argument("--port", type=int, default=8000)
    p.add_argument("--img", default="img/raspi_recover.img")
    p.add_argument("--preload", action="store_true", help="Preload image into memory")
    return p.parse_args()


async def handle_ping(request):
    # Lightweight endpoint for checking that the host server is reachable.
    return web.Response(text="OK")


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


async def init_app(img_path, preload=False):
    # Store image metadata in the aiohttp app so each request handler avoids
    # repeating stat calls and path validation.
    app = web.Application()
    if not os.path.exists(img_path):
        # Fail early so the STM32 does not see connection resets for a missing
        # backing image.
        raise SystemExit(f'image not found: {img_path}')
    size = os.path.getsize(img_path)
    app['img_path'] = img_path
    app['img_size'] = size
    if preload:
        # Loading once is useful when the image is small enough and repeated boot
        # attempts should not be affected by host filesystem cache behavior.
        print('Preloading image into memory...')
        with open(img_path, 'rb') as f:
            app['img_mem'] = f.read()
        print('Preload done, size', len(app['img_mem']))

    app.router.add_get('/ping', handle_ping)
    # `/image` is the production firmware path; `/block` is a human-friendly
    # diagnostic path.
    app.router.add_get('/image', handle_image)
    app.router.add_get('/block/{lba}', handle_block)

    return app


def main():
    args = parse_args()
    # aiohttp owns the event loop after `web.run_app`; initialization is kept in
    # an async helper so future startup checks can share the same loop style.
    app = asyncio.run(init_app(args.img, preload=args.preload))
    print(f'Serving {args.img} on http://{args.host}:{args.port} (preload={args.preload})')
    web.run_app(app, host=args.host, port=args.port)


if __name__ == '__main__':
    main()
