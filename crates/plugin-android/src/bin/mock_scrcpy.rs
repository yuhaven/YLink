//! Fake `scrcpy` for tests: records its arguments, then idles.
//!
//! `MOCK_SCRCPY_ARGS_FILE` receives the joined arguments, so a test can assert
//! the window geometry handed to the engine.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Ok(path) = std::env::var("MOCK_SCRCPY_ARGS_FILE") {
        let _ = std::fs::write(path, args.join(" "));
    }
    eprintln!("mock-scrcpy: started with {args:?}");

    let seconds = std::env::var("MOCK_ENGINE_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30);
    std::thread::sleep(std::time::Duration::from_secs(seconds));
}
