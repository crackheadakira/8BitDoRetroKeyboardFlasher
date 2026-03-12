# 8BitDo Retro Keyboard — Firmware Flash Protocol Specification

Reverse-engineered from USB pcap capture of the official 8BitDo update application.
Source of truth: `src/main.rs`.

---

## Device

| Field              | Value                                   |
| ------------------ | --------------------------------------- |
| Product            | 8BitDo Retro Keyboard                   |
| USB VID            | `0x2DC8`                                |
| USB PID            | `0x5200`                                |
| HID Usage Page     | `0x008C`                                |
| HID Usage          | `0x0001`                                |
| Interface          | 2                                       |
| MCU                | Telink TLSR82xx (RISC-V 32-bit BLE SoC) |
| Firmware ID string | `TL 8BiDo`                              |

---

## Transport

All communication uses **USB HID Interrupt transfers**:

| Direction     | Report ID | Use                        |
| ------------- | --------- | -------------------------- |
| Host → Device | `0xB2`    | Commands and firmware data |
| Device → Host | `0xB1`    | Responses and ACKs         |

Every HID report is exactly **33 bytes** (`PACKET_SIZE = 33`): 1-byte report ID
followed by 32 bytes of payload. Unused trailing bytes are zero-padded.

---

## Packet Format

### Host → Device (`0xB2`)

```
Offset  Len  Field
──────  ───  ─────────────────────────────────────────────
0       1    Report ID: 0xB2
1       1    Magic high byte: 0xAA
2       1    Magic low byte: 0x55 (handshake/finalization) or 0x56 (data + HS4)
3       1    Payload length
4       1    Type byte (0xFC, 0xFB, 0xEC, 0xF8, or 0xF4 — see per-packet)
5       1    Sequence number (uint8, wraps 0xFF → 0x01, zero is skipped)
6       1    Channel / command class
7+      N    Command-specific payload
7+N+    -    Zero padding to fill 33 bytes
```

**Magic note:** `0xAA55` is used for handshake (HS1-HS3) and finalization
packets. `0xAA56` is used for data packets and HS4.

### Device → Host (`0xB1`)

```
Offset  Len  Field
──────  ───  ─────────────────────────────────────────────
0       1    Report ID: 0xB1
1       2    Magic: 0xAA55
3       1    Payload length
4       1    Type byte
5       1    Device global sequence counter (informational, independent of host)
6       1    Response type (0xA3 = info, 0xA1 = ready/ACK)
7       1    Echo of host sequence number
8       1    Channel echo
9+      -    Response-specific payload, then zero padding
```

---

## Session Flow

```
Host                                    Device
────────────────────────────────────────────────────────
HS1 (0x60) query device info    ──────►
                                ◄────── HS1 response part 1 (33 bytes)
                                ◄────── HS1 response part 2 (33 bytes, discard)
HS2 (0x61) get capabilities     ──────►
                                ◄────── HS2 response
HS3 (0x62) enter DFU mode       ──────►
                                ◄────── HS3 response
HS4 (0x63) confirm ready        ──────►
                                ◄────── HS4 response  [byte[6]=0xA1 = flash ready]

DATA chunks (0x64) * N          ──────► (fire and forget, no per-packet ACK)

COMMIT (0x65)                   ──────►
                                ◄────── COMMIT ACK (read with 2000ms timeout)
REBOOT (0x66)                   ──────► (no response; device reboots)
```

> **Important:** HS1 produces **two** back-to-back 33-byte IN reports. The
> response filter is `buf[0]==0xB1 && buf[1]==0xAA && buf[2]==0x55 &&
buf[7]==expected_seq`. The second fragment does not match this filter and is
> discarded automatically by the read loop.

> **Important:** There is no separate VERIFY packet in this implementation.
> The COMMIT packet is sent after all data chunks. The COMMIT ACK **is** read
> (2000ms timeout) before sending REBOOT.

---

## Handshake Packets (Sequence 0x01-0x04)

Handshake sequence numbers are fixed in the packet bodies, not derived from a
counter.

### HS1 — Query Device Info

```
B2 AA 55 03 FC 01 60 60 00 00 ...
```

Response (two parts, both must be read):

- Part 1: contains device name string `TL 8BiDo` and firmware version e.g. `1.3.6r`
- Part 2: continuation fragment (first bytes are not `B1 AA 55`); discard it

### HS2 — Get Capabilities

```
B2 AA 55 03 FC 02 61 61 00 00 ...
```

### HS3 — Enter DFU Mode

```
B2 AA 55 04 FB 03 62 00 62 00 ...
```

### HS4 — Confirm Ready

```
B2 AA 56 03 FC 04 63 63 00 00 ...
```

Response `byte[6] = 0xA1` signals the device has erased flash and is ready to
receive firmware data. The first data ACK takes ~76ms (erase completes during
that window).

---

## Data Transfer (Channel `0x64`)

Sequence numbers continue from `0x05` after the handshake.

### Chunk Size

`PAYLOAD_SIZE = 16`. Firmware is split into **16-byte chunks**,
`ceil(size / 16)` packets total. The final chunk may be smaller and is
sent as-is (not zero-padded in the payload — only the HID packet itself is
zero-padded to 33 bytes).

### Data Packet Structure

```
Offset  Value
──────  ─────────────────────────────────────────────────────
0       0xB2  report ID
1       0xAA
2       0x56  magic
3       0x13  payload length = 19
4       0xEC  type (not-last) / 0xF8 type (last chunk)
5       seq   wrapping uint8, starts at 0x05, skips 0x00
6       0x64  channel
7..22   16 bytes of firmware payload
23..24  chunk trailer (2 bytes, see below)
25..32  zeros
```

**Last packet** differs in bytes 3 and 4:

- byte[3] = `0x07` (not `0x13`)
- byte[4] = `0xF8` (not `0xEC`)

### Chunk Trailer

Each data packet ends with a 2-byte trailer computed over the 16-byte payload:

```rust
fn chunk_trailer(chunk: &[u8]) -> u16 {
    let sum: u16 = chunk.iter().map(|&b| b as u16).sum();
    sum.wrapping_add(100).swap_bytes()
}
```

The trailer is appended big-endian: `packet[23] = trailer >> 8`, `packet[24] = trailer & 0xFF`.

### Timing

- No deliberate inter-packet delay is implemented.
- No per-packet ACK is read during data transfer. The device does not send
  one per chunk.
- Progress is sampled every 100 chunks by attempting a non-blocking read
  (200ms timeout) for informational status only.

---

## Sequence Number Tracking

```
Handshake:        HS1=0x01, HS2=0x02, HS3=0x03, HS4=0x04  (fixed in packet bodies)
Data start:       counter = 0x05
Increment rule:   next = counter.wrapping_add(1); if next == 0x00 { next = 0x01 }
After N chunks:   counter is 0x05 advanced N times under the above rule
Commit:           counter after last data chunk
Reboot:           counter + 1
```

The device maintains its own independent global sequence counter in
`response[5]`. This is informational only and does not affect host-side logic.

---

## Finalization

### Commit Packet (cmd `0x65`)

```
Offset  Value
──────  ──────────────────────────────────
0       0xB2
1       0xAA
2       0x55
3       0x0B  payload length
4       0xF4
5       seq
6       0x65
15      0x65
```

All other bytes are zero. The commit ACK is read with a 2000ms timeout before
proceeding. If no ACK is received, a warning is printed but execution continues.

### Reboot Packet (cmd `0x66`)

```
B2 AA 55 03 FC [seq] 66 66 00 00 ...
```

Sent immediately after the commit ACK (or timeout). The device reboots on
receipt and sends no response. Allow ~500ms for USB re-enumeration.

---

## Firmware Binary Format

- Raw Telink OTA binary, no wrapper or container.
- Transferred verbatim as 16-byte chunks.
- Magic marker `KNLT` (Telink OTA marker reversed) appears in the binary header
  at offset 8–11 (`4B 4E 4C 54`), and again near the end.
- File size is stored little-endian at offset 24 in the header.
- The last 4 bytes are a CRC32 checksum (little-endian) computed over all
  preceding bytes using the standard reflected polynomial `0xEDB88320`,
  init `0xFFFFFFFF`, **no final XOR** (non-standard — omitting the final
  `^ 0xFFFFFFFF` step).
- USB string descriptors (UTF-16LE), version strings, and BLE HID NVRAM
  library symbols (`blehid_nvram_*`) are embedded in plain text.
- Do not hardcode an expected file size — it varies between versions.

### CRC32 Reference Implementation

```python
POLY = 0xEDB88320

def firmware_crc(data: bytes) -> int:
    crc = 0xFFFFFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ POLY if crc & 1 else crc >> 1
    return crc & 0xFFFFFFFF  # no final XOR

def sign_firmware(payload: bytes) -> bytes:
    import struct
    return payload + struct.pack('<I', firmware_crc(payload))
```

---

## Reference Packet Sequence (from pcap, timestamps relative to session start)

```
t=+0.000ms  HS1 OUT  b2 aa55 03 fc 01 60 60 ...
t=+0.997ms  HS1 IN   b1 aa55 28 d7 02 a3 01 60 ...  [device info part 1]
t=+1.001ms  HS1 IN   b1 2e33 2e36 ...               [device info part 2 — discard]
t=+1.035ms  HS2 OUT  b2 aa55 03 fc 02 61 61 ...
t=+2.018ms  HS2 IN   b1 aa55 09 f6 03 a3 02 61 ...
t=+2.034ms  HS3 OUT  b2 aa55 04 fb 03 62 00 62 ...
t=+3.026ms  HS3 IN   b1 aa55 0b f4 04 a3 03 62 ...
t=+3.063ms  HS4 OUT  b2 aa56 03 fc 04 63 63 ...
t=+5.020ms  HS4 IN   b1 aa55 07 f8 05 a1 04 63 ...  [byte[6]=0xa1: flash ready]
t=+5.051ms  DATA #1  b2 aa56 13 ec 05 64 [16 bytes] [trailer] ...
t=+81.020ms DATA #1 ACK sampled (device was erasing — 76ms gap)
t=+81.071ms DATA #2  b2 aa56 13 ec 06 64 [16 bytes] [trailer] ...
            ... (remaining data packets) ...
            LAST DATA b2 aa56 07 f8 [seq] 64 [<=16 bytes] [trailer] ...
            COMMIT   b2 aa55 0b f4 [seq] 65 00 ... 65 ...
            COMMIT ACK (2000ms timeout)
            REBOOT   b2 aa55 03 fc [seq] 66 66 ...   [no ACK — device reboots]
```
