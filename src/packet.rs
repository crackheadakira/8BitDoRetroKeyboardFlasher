use crate::PACKET_SIZE;

pub struct PacketHeader {
    report_id: u8,

    magic: u16,

    payload_length: u8,

    packet_type: u8,

    channel: u8,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq)]
pub enum HandshakeStep {
    QueryDeviceInfo = 0x60,
    QueryCapabilities,
    EnterDfuMode,
    ConfirmFlashReady,
}

pub struct FirmwareChunk<'a> {
    pub firmware_bytes: &'a [u8],
    pub is_final_chunk: bool,
}

impl<'a> FirmwareChunk<'a> {
    fn calculate_chunk_checksum(&self) -> u16 {
        let sum: u16 = self.firmware_bytes.iter().map(|&b| b as u16).sum();
        sum.wrapping_add(100).swap_bytes()
    }
}

pub struct CommitPacket;

pub struct RebootPacket;

pub trait EncodePacket {
    fn header(&self) -> PacketHeader;

    fn write_payload(&self, buf: &mut [u8]);

    fn encode(&self, packet_counter: u8) -> [u8; PACKET_SIZE] {
        let mut buf = [0u8; PACKET_SIZE];

        let header = self.header();

        buf[0] = header.report_id;
        buf[1] = (header.magic >> 8) as u8;
        buf[2] = (header.magic & 0xFF) as u8;
        buf[3] = header.payload_length;
        buf[4] = header.packet_type;
        buf[5] = packet_counter;
        buf[6] = header.channel;

        self.write_payload(&mut buf[7..]);

        buf
    }
}

impl EncodePacket for HandshakeStep {
    fn header(&self) -> PacketHeader {
        PacketHeader {
            report_id: 0xB2,
            magic: match self {
                HandshakeStep::ConfirmFlashReady => 0xAA56,
                _ => 0xAA55,
            },
            payload_length: match self {
                HandshakeStep::EnterDfuMode => 0x04,
                _ => 0x03,
            },
            packet_type: match self {
                HandshakeStep::EnterDfuMode => 0xFB,
                _ => 0xFC,
            },
            channel: *self as u8,
        }
    }

    fn write_payload(&self, buf: &mut [u8]) {
        match self {
            HandshakeStep::EnterDfuMode => {
                buf[0] = 0x00;
                buf[1] = *self as u8;
            }
            _ => {
                buf[0] = *self as u8;
            }
        }
    }
}

impl<'a> EncodePacket for FirmwareChunk<'a> {
    fn header(&self) -> PacketHeader {
        PacketHeader {
            report_id: 0xB2,
            magic: 0xAA56,
            payload_length: if self.is_final_chunk { 0x07 } else { 0x13 },
            packet_type: if self.is_final_chunk { 0xF8 } else { 0xEC },
            channel: 0x64,
        }
    }

    fn write_payload(&self, buf: &mut [u8]) {
        let payload_len = self.firmware_bytes.len();

        buf[..payload_len].copy_from_slice(self.firmware_bytes);

        let checksum = self.calculate_chunk_checksum();
        buf[payload_len] = (checksum >> 8) as u8;
        buf[payload_len + 1] = (checksum & 0xFF) as u8;
    }
}

impl EncodePacket for CommitPacket {
    fn header(&self) -> PacketHeader {
        PacketHeader {
            report_id: 0xB2,
            magic: 0xAA55,
            payload_length: 0x0B,
            packet_type: 0xF4,
            channel: 0x65,
        }
    }

    fn write_payload(&self, buf: &mut [u8]) {
        buf[8] = 0x65;
    }
}

impl EncodePacket for RebootPacket {
    fn header(&self) -> PacketHeader {
        PacketHeader {
            report_id: 0xB2,
            magic: 0xAA55,
            payload_length: 0x03,
            packet_type: 0xFC,
            channel: 0x66,
        }
    }

    fn write_payload(&self, buf: &mut [u8]) {
        buf[0] = 0x66;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD_SIZE: usize = 16;

    #[test]
    fn test_chunk_trailer_zero_payload() {
        let chunk = FirmwareChunk {
            firmware_bytes: &[],
            is_final_chunk: false,
        };
        let result = chunk.calculate_chunk_checksum();
        assert_eq!(result, 0x6400);
    }

    #[test]
    fn test_chunk_trailer_known_value() {
        let payload: [u8; 16] = [
            0x0e, 0x80, 0x06, 0x03, 0x02, 0x01, 0x5d, 0x02, 0x4b, 0x4e, 0x4c, 0x54, 0xe4, 0x07,
            0x88, 0x00,
        ];
        let chunk = FirmwareChunk {
            firmware_bytes: &payload,
            is_final_chunk: false,
        };
        let sum: u16 = payload.iter().map(|&b| b as u16).sum();
        let expected = sum.wrapping_add(100).swap_bytes();
        assert_eq!(chunk.calculate_chunk_checksum(), expected);
    }

    #[test]
    fn test_chunk_trailer_wrapping() {
        let payload = [0xFFu8; 16];
        let chunk = FirmwareChunk {
            firmware_bytes: &payload,
            is_final_chunk: false,
        };
        let sum: u16 = payload.iter().map(|&b| b as u16).sum();
        let expected = sum.wrapping_add(100).swap_bytes();
        assert_eq!(chunk.calculate_chunk_checksum(), expected);
    }

    #[test]
    fn test_firmware_packet_not_last() {
        let payload = [0x01u8; PAYLOAD_SIZE];
        let chunk = FirmwareChunk {
            firmware_bytes: &payload,
            is_final_chunk: false,
        };
        let packet = chunk.encode(0x05);

        assert_eq!(packet[0], 0xB2);
        assert_eq!(packet[1], 0xAA);
        assert_eq!(packet[2], 0x56);
        assert_eq!(packet[3], 0x13);
        assert_eq!(packet[4], 0xEC);
        assert_eq!(packet[5], 0x05);
        assert_eq!(packet[6], 0x64);
        assert_eq!(&packet[7..7 + PAYLOAD_SIZE], &payload);

        let trailer = chunk.calculate_chunk_checksum();
        assert_eq!(packet[7 + PAYLOAD_SIZE], (trailer >> 8) as u8);
        assert_eq!(packet[7 + PAYLOAD_SIZE + 1], (trailer & 0xFF) as u8);

        assert_eq!(packet.len(), PACKET_SIZE);
    }

    #[test]
    fn test_firmware_packet_is_last() {
        let payload = [0x01u8; PAYLOAD_SIZE];
        let chunk = FirmwareChunk {
            firmware_bytes: &payload,
            is_final_chunk: true,
        };
        let packet = chunk.encode(0xFF);

        assert_eq!(packet[3], 0x13);
        assert_eq!(packet[4], 0xF8);
        assert_eq!(packet[5], 0xFF);
    }

    #[test]
    fn test_handshake_packets_structure() {
        let steps = [
            HandshakeStep::QueryDeviceInfo,
            HandshakeStep::QueryCapabilities,
            HandshakeStep::EnterDfuMode,
            HandshakeStep::ConfirmFlashReady,
        ];

        for (i, step) in steps.iter().enumerate() {
            let packet = step.encode((i + 1) as u8);

            assert_eq!(packet[0], 0xB2);

            // Correctly check magic bytes
            assert_eq!(packet[1], (step.header().magic >> 8) as u8); // high byte
            assert_eq!(packet[2], (step.header().magic & 0xFF) as u8); // low byte

            // Packet counter
            assert_eq!(packet[5], (i + 1) as u8);

            assert_eq!(packet.len(), PACKET_SIZE);
        }
    }

    #[test]
    fn test_reboot_packet() {
        let reboot = RebootPacket;
        let packet = reboot.encode(0x00);

        assert_eq!(packet[0], 0xB2);
        assert_eq!(packet[1], 0xAA);
        assert_eq!(packet[2], 0x55);
        assert_eq!(packet[3], 0x03);
        assert_eq!(packet[4], 0xFC);
        assert_eq!(packet[5], 0x00);
        assert_eq!(packet[6], 0x66);
        assert_eq!(packet[7], 0x66);
        assert_eq!(packet.len(), PACKET_SIZE);
    }

    #[test]
    fn test_commit_packet() {
        let commit = CommitPacket;
        let packet = commit.encode(0x00);

        assert_eq!(packet[0], 0xB2);
        assert_eq!(packet[1], 0xAA);
        assert_eq!(packet[2], 0x55);
        assert_eq!(packet[3], 0x0B);
        assert_eq!(packet[4], 0xF4);
        assert_eq!(packet[5], 0x00);
        assert_eq!(packet[6], 0x65);
        assert_eq!(packet.len(), PACKET_SIZE);
    }
}
