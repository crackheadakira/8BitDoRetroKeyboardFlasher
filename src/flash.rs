use crate::packet::EncodePacket;
use hidapi::{HidDevice, HidResult};

use crate::{PACKET_SIZE, packet::HandshakeStep};

#[derive(thiserror::Error, Debug)]
pub enum FlashError {
    #[error("Flash failed")]
    FlashFailed,

    #[error("Timed out waiting for response")]
    ResponseTimeout,

    #[error("Bad response received at {packet_counter}: {response_received:02X?}")]
    BadResponse {
        packet_counter: u8,
        response_received: [u8; 64],
    },

    #[error("[HidApi] {0}")]
    HidError(#[from] hidapi::HidError),
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
        // 4 handshakes

        const HANDSHAKE_STEPS: [HandshakeStep; 4] = [
            HandshakeStep::QueryDeviceInfo,
            HandshakeStep::QueryCapabilities,
            HandshakeStep::EnterDfuMode,
            HandshakeStep::ConfirmFlashReady,
        ];

        for (i, step) in HANDSHAKE_STEPS.into_iter().enumerate() {
            let packet = step.encode(self.packet_counter);

            self.send(&packet)?;

            let (response, n) = loop {
                let (response, n) = self.read_timeout(2000)?;
                if n == 0 {
                    return Err(FlashError::ResponseTimeout);
                }

                if response[0] == 0xB1
                    && response[1] == 0xAA
                    && response[2] == 0x55
                    && response[7] == self.packet_counter
                {
                    break (response, n);
                }
            };

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

            println!("Handshake {}/4 → {:02x?}", i + 1, &response[..n]);
            println!("ASCII: {}", ascii.trim_matches('.'));

            if response.windows(8).any(|w| w == b"TL 8BiDo") {
                println!("✓ Keyboard identified: TL 8BiDo");
            }

            self.increment_counter();
        }

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
