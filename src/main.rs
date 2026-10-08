mod collector;
mod config;
mod events;
mod herdr;
mod log;
mod model;
mod paths;
mod pricing;
mod projects;
mod transcripts;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next();
    match cmd.as_deref().unwrap_or("tui") {
        "collect" => collector::run(),
        "ensure-collector" => collector::ensure_running(),
        "--version" | "-V" => {
            println!("jarvis {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => anyhow::bail!(
            "unknown command `{other}`; expected tui | collect | ensure-collector | focus <pane>"
        ),
    }
}
