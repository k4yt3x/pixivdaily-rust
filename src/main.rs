/*
 * Copyright (C) 2021-2025 K4YT3X.
 *
 * This program is free software; you can redistribute it and/or
 * modify it under the terms of the GNU General Public License
 * as published by the Free Software Foundation; only version 2
 * of the License.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program. If not, see <https://www.gnu.org/licenses/>.
 */
use std::process;

use anyhow::Result;
use clap::Parser;
use pixivdaily::{run, Config};
use tracing::Level;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Telegram bot API token
    #[arg(short, long, env = "TELEGRAM_BOT_TOKEN", required = true)]
    token: String,

    /// ID of the chat to send messages to
    #[arg(short, long, env = "TELEGRAM_CHAT_ID", required = true)]
    chat_id: i64,

    /// number of ranking pages to fetch (18 illusts/page)
    #[arg(short, long, default_value_t = 3, env = "PIXIV_RANKING_PAGES")]
    pages: u32,

    /// run in R18 mode
    #[arg(short, long, env = "PIXIV_R18")]
    r18: bool,
}

/// parse the command line arguments and return a new
/// Config instance
///
/// # Errors
///
/// any error that implements the Error trait
///
/// # Examples
///
/// ```
/// let config = parse()?;
/// ```
fn parse() -> Result<Config> {
    let args = Args::parse();

    // assign command line values to variables
    Ok(Config::new(args.token, args.chat_id, args.pages, args.r18))
}

/// program entry point
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    // parse command line arguments into Config
    match parse() {
        Err(e) => {
            eprintln!("Program initialization error: {}", e);
            process::exit(1);
        }
        Ok(config) => process::exit(match run(config).await {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {}", e);
                1
            }
        }),
    }
}
