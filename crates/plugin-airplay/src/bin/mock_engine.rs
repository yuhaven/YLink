//! A stand-in for uxplay so plugin lifecycle can be tested without the real
//! engine. Runs for `MOCK_ENGINE_SECONDS` (default 30) and logs its arguments.

use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    eprintln!("mock-engine: started with {args:?}");

    let seconds = std::env::var("MOCK_ENGINE_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30);

    std::thread::sleep(Duration::from_secs(seconds));
    eprintln!("mock-engine: exiting cleanly");
}
