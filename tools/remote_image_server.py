#!/usr/bin/env python3
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
    p = argparse.ArgumentParser()
    p.add_argument("--host", default="192.168.1.77")
    p.add_argument("--port", type=int, default=8000)
    p.add_argument("--img", default="img/raspi_recover.img")
    p.add_argument("--preload", action="store_true", help="Preload image into memory")
    return p.parse_args()


async def handle_ping(request):
    return web.Response(text="OK")


def http_range_to_slice(range_header, file_size):
    # supports single range "bytes=start-end"
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
        headers = {
            'Content-Type': 'application/octet-stream',
            'Content-Length': str(length),
            'Content-Range': f'bytes {start}-{end}/{size}',
            'Accept-Ranges': 'bytes',
        }
        if data is not None:
            return web.Response(status=206, body=data[start:end+1], headers=headers)
        else:
            # stream from file
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
    app = web.Application()
    if not os.path.exists(img_path):
        raise SystemExit(f'image not found: {img_path}')
    size = os.path.getsize(img_path)
    app['img_path'] = img_path
    app['img_size'] = size
    if preload:
        print('Preloading image into memory...')
        with open(img_path, 'rb') as f:
            app['img_mem'] = f.read()
        print('Preload done, size', len(app['img_mem']))

    app.router.add_get('/ping', handle_ping)
    app.router.add_get('/image', handle_image)
    app.router.add_get('/block/{lba}', handle_block)

    return app


def main():
    args = parse_args()
    app = asyncio.run(init_app(args.img, preload=args.preload))
    print(f'Serving {args.img} on http://{args.host}:{args.port} (preload={args.preload})')
    web.run_app(app, host=args.host, port=args.port)


if __name__ == '__main__':
    main()
