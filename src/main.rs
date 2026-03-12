use hidapi::HidApi;

use crate::flash::FlashSession;

mod flash;
mod packet;

const VENDOR_ID: u16 = 0x2dc8;
const PRODUCT_ID: u16 = 0x5200;

const USAGE_PAGE: u16 = 0x008c;
const USAGE: u16 = 0x0001;

const PACKET_SIZE: usize = 33;
const PAYLOAD_SIZE: usize = 16;

fn send(device: &hidapi::HidDevice, data: &[u8; PACKET_SIZE]) {
    device.write(data).expect("Failed to write");
}

/// Read responses until we get the one with b1 aa55 magic that echoes
/// our sent seq in byte[7]. Discards continuation fragments (e.g. the
/// second HID report for the long HS1 device-info response).
fn read_handshake_response(device: &hidapi::HidDevice, expected_seq: u8) -> [u8; PACKET_SIZE] {
    loop {
        let mut buf = [0u8; PACKET_SIZE];
        let n = device
            .read_timeout(&mut buf, 2000)
            .expect("Failed to read during handshake");
        if n == 0 {
            panic!(
                "Timeout waiting for handshake response to seq 0x{:02x}",
                expected_seq
            );
        }

        if buf[0] == 0xb1 && buf[1] == 0xaa && buf[2] == 0x55 && buf[7] == expected_seq {
            return buf;
        }
    }
}

fn make_firmware_packet(counter: u8, payload: &[u8], is_last: bool) -> [u8; PACKET_SIZE] {
    let mut packet = [0u8; PACKET_SIZE];

    packet[0] = 0xb2;
    packet[1] = 0xaa;
    packet[2] = 0x56;
    packet[3] = if is_last { 0x07 } else { 0x13 };
    packet[4] = if is_last { 0xf8 } else { 0xec };
    packet[5] = counter;
    packet[6] = 0x64;

    let actual_len = payload.len();
    packet[7..7 + actual_len].copy_from_slice(payload);

    let trailer = chunk_trailer(payload);
    packet[7 + actual_len] = (trailer >> 8) as u8;
    packet[7 + actual_len + 1] = (trailer & 0xFF) as u8;

    packet
}

fn make_completion_packet(counter: u8) -> [u8; PACKET_SIZE] {
    let mut packet = [0u8; PACKET_SIZE];
    packet[0] = 0xb2;
    packet[1] = 0xaa;
    packet[2] = 0x55;
    packet[3] = 0x03;
    packet[4] = 0xfc;
    packet[5] = counter;
    packet[6] = 0x66;
    packet[7] = 0x66;
    packet
}

fn handshake_packets() -> [[u8; PACKET_SIZE]; 4] {
    let mut pkts = [[0u8; PACKET_SIZE]; 4];

    // Packet 1: b2 aa 55 03 fc 01 60 60 00...
    pkts[0][0] = 0xb2;
    pkts[0][1] = 0xaa;
    pkts[0][2] = 0x55;
    pkts[0][3] = 0x03;
    pkts[0][4] = 0xfc;
    pkts[0][5] = 0x01;
    pkts[0][6] = 0x60;
    pkts[0][7] = 0x60;

    // Packet 2: b2 aa 55 03 fc 02 61 61 00...
    pkts[1][0] = 0xb2;
    pkts[1][1] = 0xaa;
    pkts[1][2] = 0x55;
    pkts[1][3] = 0x03;
    pkts[1][4] = 0xfc;
    pkts[1][5] = 0x02;
    pkts[1][6] = 0x61;
    pkts[1][7] = 0x61;

    // Packet 3: b2 aa 55 04 fb 03 62 00 62 00...
    pkts[2][0] = 0xb2;
    pkts[2][1] = 0xaa;
    pkts[2][2] = 0x55;
    pkts[2][3] = 0x04;
    pkts[2][4] = 0xfb;
    pkts[2][5] = 0x03;
    pkts[2][6] = 0x62;
    pkts[2][7] = 0x00;
    pkts[2][8] = 0x62;

    // Packet 4: b2 aa 56 03 fc 04 63 63 00...
    pkts[3][0] = 0xb2;
    pkts[3][1] = 0xaa;
    pkts[3][2] = 0x56;
    pkts[3][3] = 0x03;
    pkts[3][4] = 0xfc;
    pkts[3][5] = 0x04;
    pkts[3][6] = 0x63;
    pkts[3][7] = 0x63;

    pkts
}

fn chunk_trailer(chunk: &[u8]) -> u16 {
    let sum: u16 = chunk.iter().map(|&b| b as u16).sum();
    sum.wrapping_add(100).swap_bytes()
}

fn increment_counter(counter: u8) -> u8 {
    let next = counter.wrapping_add(1);
    if next == 0x00 { 0x01 } else { next }
}

fn main() {
    println!("=== 8BitDo Retro Keyboard Flash Tool ===");
    let handshake = std::env::args().any(|a| a == "--handshake");

    let firmware = std::fs::read("patched.dat").expect("Failed to read firmware file");
    println!(
        "Firmware size: {} bytes ({} chunks)",
        firmware.len(),
        firmware.len().div_ceil(PAYLOAD_SIZE)
    );

    if handshake {
        println!("\n*** HANDSHAKE MODE - will abort before flashing ***");
    }

    let api = HidApi::new().expect("Failed to create HID API");
    let device = api
        .device_list()
        .find(|d| {
            d.vendor_id() == VENDOR_ID
                && d.product_id() == PRODUCT_ID
                && d.usage_page() == USAGE_PAGE
                && d.usage() == USAGE
        })
        .expect("Keyboard not found")
        .open_device(&api)
        .expect("Failed to open device");

    println!("Connected to keyboard\n");
    let mut flash_session = FlashSession::new(device);

    flash_session.handshake().unwrap();

    return;

    println!("[1/4] Sending handshake...");
    let packets = handshake_packets();

    for (i, packet) in packets.iter().enumerate() {
        let expected_seq = packet[5];
        send(&device, packet);
        let resp = read_handshake_response(&device, expected_seq);

        if resp[0] != 0xb1 || resp[1] != 0xaa || resp[2] != 0x55 {
            println!(
                "\nERROR: Handshake {} bad response: {:02x?}",
                i + 1,
                &resp[..4]
            );
            std::process::exit(1);
        }

        let ascii: String = resp
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("  Handshake {}/4 → {:02x?}", i + 1, &resp);
        println!("             ASCII: {}", ascii.trim_matches('.'));
        if resp.windows(8).any(|w| w == b"TL 8BiDo") {
            println!("  ✓ Keyboard identified: TL 8BiDo");
        }
    }

    let mut counter: u8 = 0x05;
    println!("  Counter after handshake: 0x{:02x}", counter);

    if handshake {
        println!("\nHandshake succeeded, keyboard is responding correctly.");
        println!("Run without --handshake to actually flash.");
        std::process::exit(0);
    }

    println!("  ✓ Handshake successful, proceeding with flash...");

    println!("\n[2/4] Sending firmware...");
    let chunks: Vec<&[u8]> = firmware.chunks(PAYLOAD_SIZE).collect();
    let total = chunks.len();
    let start = std::time::Instant::now();

    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i == total - 1;
        let packet = make_firmware_packet(counter, chunk, is_last);
        send(&device, &packet);

        if i == 0 {
            println!("first firmware packet: ${packet:02X?}");
        }

        counter = increment_counter(counter);

        let mut resp = [0u8; 64];
        if let Ok(n) = device.read_timeout(&mut resp, 200)
            && (i % 100 == 0 || i == total - 1)
        {
            let elapsed = start.elapsed().as_secs_f32();
            let bytes_sent = i * PAYLOAD_SIZE;

            let bytes_per_sec = if elapsed > 0.0 {
                bytes_sent as f32 / elapsed
            } else {
                0.0
            };

            let eta = if bytes_per_sec > 0.0 {
                (firmware.len() - bytes_sent) as f32 / bytes_per_sec
            } else {
                0.0
            };

            if n > 0 && resp[0] == 0xB1 && resp[1] == 0xAA && resp[2] == 0x55 {
                println!(
                    "{}/{} chunks ({:.1}%) | {:.0} B/s | ETA {:.1}s | status: cmd={:02x} status={:02x} seq={:02x} chan={:02x}",
                    i,
                    total,
                    (i as f32 / total as f32) * 100.0,
                    bytes_per_sec,
                    eta,
                    resp[4],
                    resp[5],
                    resp[7],
                    resp[8],
                );
            } else {
                println!(
                    "{}/{} chunks ({:.1}%) | {:.0} B/s | ETA {:.1}s | status: <no response>",
                    i,
                    total,
                    (i as f32 / total as f32) * 100.0,
                    bytes_per_sec,
                    eta,
                );
            }
        }
    }

    let elapsed = start.elapsed();
    println!(
        "  {}/{} chunks (100.0%) | Done in {:.2}s",
        total,
        total,
        elapsed.as_secs_f32()
    );

    println!("\n[3/4] Sending commit packet...");
    let mut commit = [0u8; PACKET_SIZE];

    commit[0] = 0xb2;
    commit[1] = 0xaa;
    commit[2] = 0x55;
    commit[3] = 0x0b;
    commit[4] = 0xf4;
    commit[5] = counter;
    commit[6] = 0x65;
    commit[15] = 0x65;

    send(&device, &commit);
    counter = increment_counter(counter);
    let mut acked = false;

    let mut commit_resp = [0u8; 64];
    if let Ok(n) = device.read_timeout(&mut commit_resp, 2000)
        && n >= 8
        && commit_resp[0] == 0xB1
        && commit_resp[1] == 0xAA
        && commit_resp[2] == 0x55
    {
        println!("[Commit acknowledged]");
        acked = true;
    }

    if !acked {
        println!("Warning: Commit not acknowledged, flash may fail!");
    }

    println!("\n[4/4] Sending completion packet...");
    let completion = make_completion_packet(counter);
    send(&device, &completion);

    println!("\n=== Flash complete ===");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_trailer_zero_payload() {
        // sum=0, +100=100, swap_bytes(100) = swap_bytes(0x0064) = 0x6400
        let result = chunk_trailer(&[]);
        assert_eq!(result, 0x6400);
    }

    #[test]
    fn test_chunk_trailer_known_value() {
        // sum of first 16 bytes of KNLT header
        let payload: [u8; 16] = [
            0x0e, 0x80, 0x06, 0x03, 0x02, 0x01, 0x5d, 0x02, 0x4b, 0x4e, 0x4c, 0x54, 0xe4, 0x07,
            0x88, 0x00,
        ];
        let sum: u16 = payload.iter().map(|&b| b as u16).sum();
        let expected = sum.wrapping_add(100).swap_bytes();
        assert_eq!(chunk_trailer(&payload), expected);
    }

    #[test]
    fn test_chunk_trailer_wrapping() {
        // All 0xFF bytes should wrap
        let payload = [0xFFu8; 16];
        let sum: u16 = payload.iter().map(|&b| b as u16).sum(); // 0x0FF0
        let expected = sum.wrapping_add(100).swap_bytes();
        assert_eq!(chunk_trailer(&payload), expected);
    }

    #[test]
    fn test_firmware_packet_not_last() {
        let payload = [0x01u8; PAYLOAD_SIZE];
        let packet = make_firmware_packet(0x05, &payload, false);

        assert_eq!(packet[0], 0xb2);
        assert_eq!(packet[1], 0xaa);
        assert_eq!(packet[2], 0x56);
        assert_eq!(packet[3], 0x13); // not last
        assert_eq!(packet[4], 0xec); // not last
        assert_eq!(packet[5], 0x05); // counter
        assert_eq!(packet[6], 0x64);
        assert_eq!(packet[7..7 + PAYLOAD_SIZE], payload);

        // verify trailer bytes
        let trailer = chunk_trailer(&payload);
        assert_eq!(packet[7 + PAYLOAD_SIZE], (trailer >> 8) as u8);
        assert_eq!(packet[7 + PAYLOAD_SIZE + 1], (trailer & 0xFF) as u8);

        assert_eq!(packet.len(), PACKET_SIZE);
    }

    #[test]
    fn test_firmware_packet_is_last() {
        let payload = [0x01u8; PAYLOAD_SIZE];
        let packet = make_firmware_packet(0xff, &payload, true);

        assert_eq!(packet[3], 0x07); // is_last
        assert_eq!(packet[4], 0xf8); // is_last
        assert_eq!(packet[5], 0xff); // counter
    }

    #[test]
    fn test_firmware_packet_matches_wireshark() {
        // First 16 bytes of the KNLT firmware header as seen in wireshark capture
        let first_chunk: [u8; PAYLOAD_SIZE] = [
            0x0e, 0x80, 0x06, 0x03, 0x02, 0x01, 0x5d, 0x02, 0x4b, 0x4e, 0x4c, 0x54, 0xe4, 0x07,
            0x88, 0x00,
        ];
        let packet = make_firmware_packet(0x05, &first_chunk, false);

        // Header fields
        assert_eq!(&packet[0..3], &[0xb2, 0xaa, 0x56]);
        assert_eq!(packet[3], 0x13);
        assert_eq!(packet[4], 0xec);
        assert_eq!(packet[5], 0x05);
        assert_eq!(packet[6], 0x64);

        // Payload
        assert_eq!(&packet[7..7 + PAYLOAD_SIZE], &first_chunk);

        // Trailer
        let trailer = chunk_trailer(&first_chunk);
        assert_eq!(packet[7 + PAYLOAD_SIZE], (trailer >> 8) as u8);
        assert_eq!(packet[7 + PAYLOAD_SIZE + 1], (trailer & 0xFF) as u8);
    }

    #[test]
    fn test_completion_packet() {
        let packet = make_completion_packet(0x85);
        assert_eq!(packet[0], 0xb2);
        assert_eq!(packet[1], 0xaa);
        assert_eq!(packet[2], 0x55);
        assert_eq!(packet[3], 0x03);
        assert_eq!(packet[4], 0xfc);
        assert_eq!(packet[5], 0x85);
        assert_eq!(packet[6], 0x66);
        assert_eq!(packet[7], 0x66);
        assert_eq!(packet.len(), PACKET_SIZE);
    }

    #[test]
    fn test_increment_counter_normal() {
        assert_eq!(increment_counter(0x05), 0x06);
        assert_eq!(increment_counter(0xfe), 0xff);
    }

    #[test]
    fn test_increment_counter_wraps_skip_zero() {
        // 0xff wraps to 0x00, but 0x00 is skipped so should return 0x01
        assert_eq!(increment_counter(0xff), 0x01);
    }

    #[test]
    fn test_increment_counter_from_zero() {
        assert_eq!(increment_counter(0x00), 0x01);
    }

    #[test]
    fn test_handshake_packets_structure() {
        let packets = handshake_packets();
        assert_eq!(packets.len(), 4);

        // All packets share the b2 aa prefix
        for pkt in &packets {
            assert_eq!(pkt[0], 0xb2);
            assert_eq!(pkt[1], 0xaa);
            assert_eq!(pkt.len(), PACKET_SIZE);
        }

        // Seq numbers 1-4
        assert_eq!(packets[0][5], 0x01);
        assert_eq!(packets[1][5], 0x02);
        assert_eq!(packets[2][5], 0x03);
        assert_eq!(packets[3][5], 0x04);

        // Packet 1: 55 variant
        assert_eq!(packets[0][2], 0x55);
        assert_eq!(packets[0][3], 0x03);
        assert_eq!(packets[0][4], 0xfc);
        assert_eq!(packets[0][6], 0x60);
        assert_eq!(packets[0][7], 0x60);

        // Packet 2: 55 variant
        assert_eq!(packets[1][2], 0x55);
        assert_eq!(packets[1][6], 0x61);
        assert_eq!(packets[1][7], 0x61);

        // Packet 3: longer payload (04 fb), has extra byte
        assert_eq!(packets[2][2], 0x55);
        assert_eq!(packets[2][3], 0x04);
        assert_eq!(packets[2][4], 0xfb);
        assert_eq!(packets[2][6], 0x62);
        assert_eq!(packets[2][7], 0x00);
        assert_eq!(packets[2][8], 0x62);

        // Packet 4: 56 variant
        assert_eq!(packets[3][2], 0x56);
        assert_eq!(packets[3][6], 0x63);
        assert_eq!(packets[3][7], 0x63);
    }

    #[test]
    fn test_payload_size_fits_in_packet() {
        // 7 header bytes + PAYLOAD_SIZE + 2 trailer bytes must fit in PACKET_SIZE
        assert!(7 + PAYLOAD_SIZE + 2 <= PACKET_SIZE);
    }
}
