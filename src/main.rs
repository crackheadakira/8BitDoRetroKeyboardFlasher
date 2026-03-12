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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let firmware_path = args
        .iter()
        .find(|a| !a.starts_with("--") && a.ends_with(".dat"))
        .unwrap_or_else(|| {
            eprintln!("Usage: flash <firmware.dat> [--handshake]");
            std::process::exit(1);
        });

    let handshake = args.iter().any(|a| a == "--handshake");

    let firmware = std::fs::read(firmware_path).unwrap_or_else(|e| {
        eprintln!("Failed to read {firmware_path}: {e}");
        std::process::exit(1);
    });

    println!("=== 8BitDo Retro Keyboard Flash Tool ===");
    println!(
        "Firmware: {firmware_path} ({} bytes, {} chunks)",
        firmware.len(),
        firmware.len().div_ceil(PAYLOAD_SIZE)
    );

    if handshake {
        println!("Mode: Handshake only");
    }

    let api = HidApi::new().unwrap_or_else(|e| {
        eprintln!("Failed to create HID API: {e}");
        std::process::exit(1);
    });

    let device = api
        .device_list()
        .find(|d| {
            d.vendor_id() == VENDOR_ID
                && d.product_id() == PRODUCT_ID
                && d.usage_page() == USAGE_PAGE
                && d.usage() == USAGE
        })
        .unwrap_or_else(|| {
            eprintln!("Keyboard not found. Is it connected?");
            std::process::exit(1);
        })
        .open_device(&api)
        .unwrap_or_else(|e| {
            eprintln!("Failed to open device: {e}");
            std::process::exit(1);
        });

    println!("Connected to keyboard");

    let mut session = FlashSession::new(device);

    session.handshake().unwrap_or_else(|e| {
        eprintln!("Handshake failed: {e}");
        std::process::exit(1);
    });

    if handshake {
        println!("Handshake succeeded. Run without --handshake to flash.");
        return;
    }

    session.firmware(firmware).unwrap_or_else(|e| {
        eprintln!("Firmware transfer failed: {e}");
        std::process::exit(1);
    });

    session.commit().unwrap_or_else(|e| {
        eprintln!("Commit failed: {e}");
        std::process::exit(1);
    });

    session.reboot().unwrap_or_else(|e| {
        eprintln!("Reboot failed: {e}");
        std::process::exit(1);
    });

    println!("Flash complete.");
}
