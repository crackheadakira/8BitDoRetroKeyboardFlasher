use crate::{
    PAYLOAD_SIZE,
    packet::{CommitPacket, EncodePacket, FirmwareChunk, RebootPacket},
};
use hidapi::{HidDevice, HidResult};

use crate::{PACKET_SIZE, packet::HandshakeStep};

#[derive(thiserror::Error, Debug)]
pub enum FlashError {
    #[error("[HidApi] {0}")]
    HidError(#[from] hidapi::HidError),

    #[error("Timed out waiting for response")]
    ResponseTimeout,

    #[error("Device not recognized. Is this an 8BitDo Retro Keyboard?")]
    DeviceNotRecognized,

    #[error("Commit not acknowledged, flash may fail so not sending final packet")]
    CommitNotAcknowledged,
}

pub struct FlashSession {
    device: HidDevice,

    packet_counter: u8,
}

impl FlashSession {
    pub fn new(device: HidDevice) -> Self {
        Self {
            device,
            packet_counter: 0x01,
        }
    }

    pub fn handshake(&mut self) -> Result<(), FlashError> {
        const HANDSHAKE_STEPS: [HandshakeStep; 4] = [
            HandshakeStep::QueryDeviceInfo,
            HandshakeStep::QueryCapabilities,
            HandshakeStep::EnterDfuMode,
            HandshakeStep::ConfirmFlashReady,
        ];

        let mut keyboard_identified = false;
        for (i, step) in HANDSHAKE_STEPS.into_iter().enumerate() {
            let packet = step.encode(self.packet_counter);

            self.send(&packet)?;

            let response = loop {
                let (response, n) = self.read_timeout(2000)?;
                if n == 0 {
                    return Err(FlashError::ResponseTimeout);
                }

                if response[0] == 0xB1
                    && response[1] == 0xAA
                    && response[2] == 0x55
                    && response[7] == self.packet_counter
                {
                    break response;
                }
            };

            if response.windows(8).any(|w| w == b"TL 8BiDo") {
                println!("Keyboard identified: TL 8BiDo");
                keyboard_identified = true;
            }

            let ascii: String = response
                .iter()
                .map(|&b| {
                    if b.is_ascii_graphic() || b == b' ' {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();

            println!(
                "Handshake {}/4 OK, ASCII: {}",
                i + 1,
                ascii.trim_matches('.')
            );

            self.increment_counter();
        }

        if !keyboard_identified {
            return Err(FlashError::DeviceNotRecognized);
        }

        Ok(())
    }

    pub fn firmware(&mut self, firmware: Vec<u8>) -> Result<(), FlashError> {
        let chunks: Vec<&[u8]> = firmware.chunks(PAYLOAD_SIZE).collect();
        let total_chunks = chunks.len();

        let start = std::time::Instant::now();

        for (i, chunk) in chunks.iter().enumerate() {
            let is_last_chunk = total_chunks - 1 == i;
            let packet = FirmwareChunk {
                firmware_bytes: chunk,
                is_final_chunk: is_last_chunk,
            }
            .encode(self.packet_counter);

            self.send(&packet)?;
            self.increment_counter();

            let (response, n) = self.read_timeout(200)?;

            if i % 100 == 0 {
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

                if n > 0 && response[0] == 0xB1 && response[1] == 0xAA && response[2] == 0x55 {
                    println!(
                        "{i}/{total_chunks} chunks ({:.1}%) | {bytes_per_sec:.0} B/s | ETA {eta:.1}s | status: cmd={:02X} status={:02X} counter={:02X} channel={:02X}",
                        (i as f32 / total_chunks as f32) * 100.0,
                        response[4],
                        response[5],
                        response[7],
                        response[8],
                    );
                } else {
                    println!(
                        "{i}/{total_chunks} chunks ({:.1}%) | {bytes_per_sec:.0} B/s | ETA {eta:.1}s | status: <no response>",
                        (i as f32 / total_chunks as f32) * 100.0,
                    );
                }
            }
        }

        let elapsed = start.elapsed();
        println!(
            "{total_chunks}/{total_chunks} chunks (100.0%) | Done in {:.2}s",
            elapsed.as_secs_f32()
        );

        Ok(())
    }

    pub fn commit(&mut self) -> Result<(), FlashError> {
        let packet = CommitPacket.encode(self.packet_counter);

        self.send(&packet)?;
        self.increment_counter();

        let mut acked = false;

        let (response, n) = self.read_timeout(2000)?;

        if n >= 8 && response[0] == 0xB1 && response[1] == 0xAA && response[2] == 0x55 {
            println!("Commit acknowledged");
            acked = true;
        }

        if !acked {
            return Err(FlashError::CommitNotAcknowledged);
        }

        Ok(())
    }

    pub fn reboot(&mut self) -> Result<(), FlashError> {
        let packet = RebootPacket.encode(self.packet_counter);
        self.send(&packet)?;

        Ok(())
    }

    fn read_timeout(&self, timeout: i32) -> HidResult<([u8; 64], usize)> {
        let mut response = [0u8; 64];
        let n = self.device.read_timeout(&mut response, timeout)?;

        Ok((response, n))
    }

    fn increment_counter(&mut self) {
        self.packet_counter = match self.packet_counter.wrapping_add(1) {
            0x00 => 0x01,
            n => n,
        };
    }

    fn send(&self, data: &[u8; PACKET_SIZE]) -> HidResult<usize> {
        self.device.write(data)
    }
}
