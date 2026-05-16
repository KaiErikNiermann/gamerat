use anyhow::Result;
use clap::Parser as _;

fn main() -> Result<()> {
    let args = gamerat_daemon::Args::parse();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(gamerat_daemon::run(args))
}
