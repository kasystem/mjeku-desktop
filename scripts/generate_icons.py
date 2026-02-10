import os
import struct
import zlib


def _png_chunk(chunk_type: bytes, data: bytes) -> bytes:
    crc = zlib.crc32(chunk_type)
    crc = zlib.crc32(data, crc) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + chunk_type + data + struct.pack(">I", crc)


def make_rgba_png(width: int, height: int, rgba: tuple[int, int, int, int]) -> bytes:
    r, g, b, a = rgba
    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)

    # filter byte per row + raw RGBA pixels
    row = bytes([0]) + bytes([r, g, b, a]) * width
    raw = row * height
    comp = zlib.compress(raw, level=9)

    return b"".join(
        [
            sig,
            _png_chunk(b"IHDR", ihdr),
            _png_chunk(b"IDAT", comp),
            _png_chunk(b"IEND", b""),
        ]
    )


def make_ico_from_png(png_bytes: bytes, width: int, height: int) -> bytes:
    # ICO header: reserved(0), type(1), count(1)
    header = struct.pack("<HHH", 0, 1, 1)

    # Directory entry (16 bytes)
    w = 0 if width >= 256 else width
    h = 0 if height >= 256 else height
    color_count = 0
    reserved = 0
    planes = 1
    bit_count = 32
    bytes_in_res = len(png_bytes)
    image_offset = 6 + 16
    entry = struct.pack(
        "<BBBBHHII",
        w,
        h,
        color_count,
        reserved,
        planes,
        bit_count,
        bytes_in_res,
        image_offset,
    )

    return header + entry + png_bytes


def main() -> None:
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    icons_dir = os.path.join(repo_root, "src-tauri", "icons")
    os.makedirs(icons_dir, exist_ok=True)

    # A simple teal square.
    width, height = 256, 256
    png = make_rgba_png(width, height, (15, 118, 110, 255))
    ico = make_ico_from_png(png, width, height)

    with open(os.path.join(icons_dir, "icon.png"), "wb") as f:
        f.write(png)
    with open(os.path.join(icons_dir, "icon.ico"), "wb") as f:
        f.write(ico)

    print("Wrote src-tauri/icons/icon.png")
    print("Wrote src-tauri/icons/icon.ico")


if __name__ == "__main__":
    main()

