# 8BitDo Retro Keyboard — Firmware Flash Protocol Specification

Reverse-engineered from USB pcap capture of the official 8BitDo update application.
Source of truth: `src/main.rs`.

---

## Device

| Field              | Value                               |
| ------------------ | ----------------------------------- |
| Product            | 8BitDo Retro Keyboard               |
| USB VID            | `0x2DC8`                            |
| USB PID            | `0x5200`                            |
| HID Usage Page     | `0x008C`                            |
| HID Usage          | `0x0001`                            |
| Interface          | 2                                   |
| MCU                | Telink TLSR82xx (TC32 architecture) |
| Firmware ID string | `TL 8BiDo`                          |

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
0       1    Report ID: 0xB2 <-- This is stripped in firmware
1       1    0xAA (purpose unknown)
2       1    0x55 or 0x56 (purpose unknown, varies by packet type)
3       1    Length (see below)
4       1    Bitwise complement of length
5       1    Sequence number (uint8, wraps 0xFF → 0x01, zero is skipped)
6       1    Command/channel (0x60-0x63 handshake, 0x64 data, 0x65 commit, 0x66 reboot)
7+      N    Command-specific payload
7+N+    -    Zero padding to fill 33 bytes
```

**byte[1]/byte[2]:** `0xAA55` is used for handshake packets (HS1-HS3) and
finalization packets (commit, reboot). `0xAA56` is used for data packets and HS4.
Purpose of these bytes is unknown.

**Length field (byte[3]):** Total bytes after the sequence number (byte[5]),
i.e. `channel(1) + payload_bytes(N) + trailer_bytes`. For non-data packets the
trailer is absent (0 bytes). For data packets the trailer is 2 bytes. The
formula holds consistently across all observed packets:

| Packet  | channel | payload | trailer | length |
| ------- | ------- | ------- | ------- | ------ |
| HS1-HS4 | 1       | 1       | 0       | `0x03` |
| HS3     | 1       | 2       | 0       | `0x04` |
| Data    | 1       | 16      | 2       | `0x13` |
| Last    | 1       | N≤16    | 2       | N+3    |
| Commit  | 1       | 9       | 0       | `0x0B` |
| Reboot  | 1       | 1       | 0       | `0x03` |

| Value  | Context               |
| ------ | --------------------- |
| `0xFC` | HS1, HS2, HS4, Reboot |
| `0xFB` | HS3                   |
| `0xEC` | Data (not last chunk) |
| `0xF8` | Data (last chunk)     |
| `0xF4` | Commit                |

### Device → Host (`0xB1`)

```
Offset  Len  Field
──────  ───  ─────────────────────────────────────────────
0       1    Report ID: 0xB1
1       1    0xAA
2       1    0x55
3       1    Length (same formula as host packets)
4       1    field_4 (purpose unknown)
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

> **Important:** There is no separate VERIFY packet. The COMMIT packet is sent
> after all data chunks. The COMMIT ACK **is** read (2000ms timeout) before
> sending REBOOT.

---

## Handshake Packets (Sequence 0x01–0x04)

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
2       0x56
3       len   channel(1) + payload(N) + trailer(2) = N+3
4       0xEC  field_4: not-last chunk / 0xF8: last chunk
5       seq   wrapping uint8, starts at 0x05, skips 0x00
6       0x64  channel
7..22   16 bytes of firmware payload (fewer for last chunk)
23..24  chunk trailer (2 bytes, see below)
25..32  zeros
```

### Chunk Trailer

Each data packet ends with a 2-byte trailer computed over the chunk payload:

```rust
fn chunk_trailer(chunk: &[u8]) -> u16 {
    let sum: u16 = chunk.iter().map(|&b| b as u16).sum();
    sum.wrapping_add(100).swap_bytes()
}
```

Appended big-endian: `packet[7+N] = trailer >> 8`, `packet[7+N+1] = trailer & 0xFF`.

### Timing

- No deliberate inter-packet delay is implemented.
- No per-packet ACK is read during data transfer.
- Progress is sampled every 100 chunks by attempting a non-blocking read
  (200ms timeout) for informational status only.

---

## Sequence Number Tracking

```
Handshake:        HS1=0x01, HS2=0x02, HS3=0x03, HS4=0x04
Data start:       counter = 0x05
Increment rule:   next = counter.wrapping_add(1); if next == 0x00 { next = 0x01 }
Commit:           counter after last data chunk
Reboot:           counter after commit
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
3       0x0B  length: channel(1) + payload(9) = 10... off by 1, reason unknown
4       0xF4
5       seq
6       0x65  channel
7..14   0x00
15      0x65
16..32  0x00
```

> **Note:** The commit length field (`0x0B` = 11) is 1 more than the formula
> predicts (`channel(1) + payload(9)` = 10). The reason for this discrepancy
> is unknown.

The commit ACK is read with a 2000ms timeout. If no ACK is received the flash
may be incomplete — the reboot packet is not sent.

### Reboot Packet (cmd `0x66`)

```
B2 AA 55 03 FC [seq] 66 66 00 00 ...
```

Sent immediately after the commit ACK. The device reboots on receipt and sends
no response. Allow ~500ms for USB re-enumeration.

---

## Firmware Binary Format

- Raw Telink OTA binary, no wrapper or container.
- Transferred verbatim as 16-byte chunks.
- Magic marker `KNLT` (`4B 4E 4C 54`) at offset 8–11 in the file header.
- File size stored little-endian at offset 24 in the header.
- Last 4 bytes: CRC32 checksum (little-endian) over all preceding bytes.
  Algorithm: standard reflected CRC32, poly `0xEDB88320`, init `0xFFFFFFFF`,
  **no final XOR** (non-standard).
- USB string descriptors (UTF-16LE), version strings, and BLE HID NVRAM
  symbols (`blehid_nvram_*`) are embedded in plain text.
- File size varies between versions — do not hardcode an expected size.

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
t=+81.020ms DATA ACK sampled (device was erasing — 76ms gap)
t=+81.071ms DATA #2  b2 aa56 13 ec 06 64 [16 bytes] [trailer] ...
            ... (remaining data packets) ...
            LAST     b2 aa56 [N+3] f8 [seq] 64 [<=16 bytes] [trailer] ...
            COMMIT   b2 aa55 0b f4 [seq] 65 00 00 00 00 00 00 00 00 65 ...
            COMMIT ACK (2000ms timeout)
            REBOOT   b2 aa55 03 fc [seq] 66 66 ...   [no ACK — device reboots]
```
