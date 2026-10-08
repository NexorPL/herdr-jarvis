mod config;
mod herdr;
mod log;
mod paths;
mod pricing;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next();
    match cmd.as_deref().unwrap_or("tui") {
        "--version" | "-V" => {
            println!("jarvis {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => anyhow::bail!(
            "unknown command `{other}`; expected tui | collect | ensure-collector | focus <pane>"
        ),
    }
}
