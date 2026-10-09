mod collector;
mod config;
mod events;
mod herdr;
mod ideas;
mod log;
mod model;
mod paths;
mod pricing;
mod projects;
mod run;
mod store;
mod transcripts;
mod ui;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next();
    match cmd.as_deref().unwrap_or("tui") {
        "tui" => ui::run(),
        "collect" => collector::run(),
        "ensure-collector" => collector::ensure_running(),
        "focus" => {
            let pane = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("usage: jarvis focus <pane_id>"))?;
            std::thread::sleep(std::time::Duration::from_millis(300));
            herdr::focus_pane(&herdr::socket_path(), &pane)
        }
        "--version" | "-V" => {
            println!("jarvis {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => anyhow::bail!(
            "unknown command `{other}`; expected tui | collect | ensure-collector | focus <pane>"
        ),
    }
}
