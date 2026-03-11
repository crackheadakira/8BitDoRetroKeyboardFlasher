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
payload = data[:-4]  # strip existing checksum
crc = firmware_crc(payload)
open(path, 'wb').write(payload + struct.pack('<I', crc))
print(f'Replaced CRC: {crc:08x} in {path}')
