//! Console companion for CastPane.
//!
//! The GUI binary is a Windows-subsystem program, so its stdout is awkward to
//! capture from a terminal. This one is a normal console program: pipes and
//! redirection work the way people expect.
//!
//!     CastPane-cli.exe --diagnose > report.txt
//!     CastPane-cli.exe --diagnose

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let code = match args.first().map(String::as_str) {
        Some("--diagnose") => castpane_lib::headless_diagnose(),
        Some("--version") | Some("-V") => {
            println!("CastPane {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("CastPane CLI");
            eprintln!("  --diagnose   逐个插件自检，退出码 0 表示全部通过");
            eprintln!("  --version    打印版本");
            2
        }
    };

    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
