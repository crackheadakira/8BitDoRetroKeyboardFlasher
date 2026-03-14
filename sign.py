import struct
import sys

POLY = 0xEDB88320

def firmware_crc(data: bytes) -> int:
    crc = 0xFFFFFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ POLY if crc & 1 else crc >> 1
    return crc & 0xFFFFFFFF

path = sys.argv[1]
data = open(path, 'rb').read()
crc = firmware_crc(data)
with open(path, 'ab') as f:
    f.write(struct.pack('<I', crc))
print(f'Appended CRC: {crc:08x} to {path}')
