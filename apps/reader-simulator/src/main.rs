use std::process::ExitCode;

use transitguard_device_protocol::DeviceProtocolVersion;
use transitguard_domain::ReaderId;
use transitguard_reader_simulator::{ReaderConnectivity, ReaderSimulator};

fn main() -> ExitCode {
    let reader_result = ReaderSimulator::new(
        ReaderId::generate(),
        DeviceProtocolVersion::CURRENT,
        env!("CARGO_PKG_VERSION"),
    );

    let Ok(mut reader) = reader_result else {
        eprintln!("failed to construct TransitGuard reader simulator");

        return ExitCode::FAILURE;
    };

    if let Err(error) = reader.start(ReaderConnectivity::Connected) {
        eprintln!("failed to start TransitGuard reader simulator: {error}");

        return ExitCode::FAILURE;
    }

    let health = reader.health_snapshot();

    println!("TransitGuard reader simulator started");
    println!("reader_id={}", health.reader_id);
    println!("state={:?}", health.state);
    println!(
        "device_protocol_version={}",
        health.protocol_version.value()
    );
    println!("software_version={}", health.software_version);
    println!("next_local_sequence={}", health.next_local_sequence);

    ExitCode::SUCCESS
}
