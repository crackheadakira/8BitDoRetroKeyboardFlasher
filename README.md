# 8BitDo Retro Keyboard Flasher

A reverse-engineered firmware flash tool for the **8BitDo Retro Keyboard**, written in Rust. Communicates directly with the keyboard over USB HID without requiring the official 8BitDo software.

---

## Background

The official 8BitDo Ultimate Software is required to flash firmware updates to the Retro Keyboard. This project reverse engineers the USB HID protocol used during flashing, enabling custom firmware to be written and flashed directly from the command line — on any OS.

The protocol was recovered from a Wireshark USB pcap capture of the official application. The firmware format was reverse engineered from the `.dat` files served by 8BitDo's firmware API.

## Features

- Full handshake + flash + commit + reboot sequence over USB HID
- No dependency on official 8BitDo software
- Works on Linux (and anywhere `hidapi` is supported)
- `--handshake` flag to test device connectivity without flashing
- Progress reporting during transfer

## Findings

### Firmware Format

The `.dat` files are raw Telink OTA binaries with no encryption. They contain:

- A `KNLT` magic header at offset 8 (Telink OTA marker)
- File size stored little-endian at offset 24
- Plain-text version strings, USB descriptors, and BLE HID SDK symbols
- A **CRC32 checksum** in the final 4 bytes (little-endian)

The CRC32 uses standard reflected polynomial `0xEDB88320`, init `0xFFFFFFFF`, and no final XOR — this differs from the standard `zlib.crc32` which applies a final `^ 0xFFFFFFFF`.

### Flash Protocol

Communication uses USB HID interrupt transfers on the vendor interface (`Usage Page 0x008C`, `Usage 0x0001`). Every report is exactly 33 bytes.

See [`SPEC.md`](SPEC.md) for the full protocol specification.

Key points:

- 4-packet handshake sequence before flashing
- Firmware sent in **16-byte chunks** with a per-chunk trailer: `swap_bytes(sum(chunk) + 100)`
- No per-packet ACK during data transfer — the device buffers the full firmware
- A single COMMIT packet triggers the actual write to flash
- REBOOT packet sent immediately after COMMIT ACK

## Usage

### Prerequisites

### Build

```bash
cargo build --release
```

### Flash

Place your firmware file as `patched.dat` in the working directory, then:

```bash
# Test connectivity only (no flash)
cargo run --release -- --handshake

# Flash firmware
cargo run --release
```

### Sign a patched firmware

After modifying a firmware binary, recalculate the CRC32 trailer:

```python
import struct, sys

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
payload = data[:-4]
crc = firmware_crc(payload)
open(path, 'wb').write(payload + struct.pack('<I', crc))
print(f'Signed: {crc:08x}')
```

```bash
python3 sign.py your_firmware.dat
```

## Device

| Field          | Value           |
| -------------- | --------------- |
| USB VID        | `0x2DC8`        |
| USB PID        | `0x5200`        |
| HID Usage Page | `0x008C`        |
| HID Usage      | `0x0001`        |
| MCU            | Telink TLSR82xx |
| ID string      | `TL 8BiDo`      |

## Project Status

- [x] Protocol reverse engineered
- [x] Firmware format reverse engineered
- [x] CRC32 checksum algorithm confirmed
- [x] Flash tool working end-to-end

## Related Work

- [8bitdo-kbd-mapper](https://github.com/goncalor/8bitdo-kbd-mapper) - HID-based key remapping tool (no firmware modification required)
- [8BitDo Numpad firmware hackability](https://skowronski.tech/posts/2025-05-24-8bitdo-numpad-firmware-hackability) - similar research on the Retro Numpad (different MCU config)
- [8bitdo-firmware](https://github.com/fwupd/8bitdo-firmware) - Firmware for 8BitDo controllers

## Disclaimer

This project is not affiliated with 8BitDo. Flashing unofficial firmware may void your warranty. The authors take no responsibility for bricked devices.
