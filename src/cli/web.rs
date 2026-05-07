use std::net::SocketAddr;

use async_trait::async_trait;

use crate::cli::{CommandContext, CommandTrait};
use crate::web::{WebOptions, run};

#[derive(clap::Args, Clone, Debug)]
/// Launch the browser-based UI (mirrors `ins tui`).
pub struct WebArgs {
    /// Bind address. Default 127.0.0.1:7878. Use `0.0.0.0:0` to listen on all interfaces with a kernel-allocated port.
    #[arg(long, default_value = "127.0.0.1:7878")]
    pub bind: SocketAddr,

    /// Do not auto-open a browser window.
    #[arg(long)]
    pub no_open: bool,

    /// Static auth token for non-loopback binds. Auto-generated if omitted.
    #[arg(long)]
    pub token: Option<String>,
}

pub struct WebCommand;

#[async_trait]
impl CommandTrait for WebCommand {
    type Args = WebArgs;

    async fn run(args: WebArgs, ctx: CommandContext) -> anyhow::Result<()> {
        let token = if args.bind.ip().is_loopback() {
            None
        } else {
            Some(args.token.unwrap_or_else(generate_token))
        };
        run(
            ctx.home,
            ctx.config,
            WebOptions {
                bind: args.bind,
                no_open: args.no_open,
                token,
            },
        )
        .await
    }
}

fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
