use std::process::ExitCode;

use transitguard_reader_simulator::run_demo_scenario;

fn main() -> ExitCode {
    let report = match run_demo_scenario() {
        Ok(report) => report,

        Err(error) => {
            eprintln!(
                "failed to run TransitGuard reader demonstration: {error}"
            );

            return ExitCode::FAILURE;
        }
    };

    let output = match serde_json::to_string_pretty(&report) {
        Ok(output) => output,

        Err(error) => {
            eprintln!(
                "failed to serialize TransitGuard reader demonstration: {error}"
            );

            return ExitCode::FAILURE;
        }
    };

    println!("{output}");

    ExitCode::SUCCESS
}
