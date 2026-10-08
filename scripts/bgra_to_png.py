"""Convert a native premultiplied BGRA8 offscreen buffer to PNG, without dependencies."""
import argparse
import binascii
from pathlib import Path
import struct
import zlib

def convert(data: bytes, width: int, height: int) -> bytes:
    if width <= 0 or height <= 0 or len(data) != width * height * 4:
        raise ValueError('Dimensions do not match BGRA buffer length')
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        for x in range(width):
            i = (y * width + x) * 4
            b, g, r, a = data[i:i+4]
            if a:
                raw.extend((min(255, (c*255+a//2)//a) for c in (r,g,b)))
            else:
                raw.extend((0,0,0))
            raw.append(a)
    def chunk(kind, body):
        return struct.pack('!I',len(body))+kind+body+struct.pack('!I',binascii.crc32(kind+body)&0xffffffff)
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!IIBBBBB',width,height,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(raw,9))+chunk(b'IEND',b'')

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('output',type=Path)
    parser.add_argument('--width',type=int,default=960)
    parser.add_argument('--height',type=int,default=680)
    args=parser.parse_args()
    args.output.write_bytes(convert(args.source.read_bytes(),args.width,args.height))
