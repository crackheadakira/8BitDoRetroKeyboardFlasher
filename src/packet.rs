use crate::{PACKET_SIZE, packet};

pub struct PacketHeader {
    report_id: u8,

    magic: u16,

    payload_length: u8,

    packet_type: u8,

    channel: u8,
}

pub enum Packet<'a> {
    Handshake(HandshakeStep),
    FirmwareData(FirmwareChunk<'a>),
    FlashCommit(CommitPacket),
    Reboot(RebootPacket),
}

#[repr(u8)]
#[derive(Clone, Copy)]
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
