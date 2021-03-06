/*
 * Copyright (C) 2021-2022 K4YT3X.
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
use std::{process, sync::Mutex};

use anyhow::Result;
use clap::{Arg, Command};
use pixivdaily::{run, Config, VERSION};
use slog::{o, Drain};

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
    // parse command line arguments
    let matches = Command::new("pixivdaily")
        .version(VERSION)
        .author("K4YT3X <i@k4yt3x.com>")
        .about("A Telegram bot that posts Pixiv's daily rankings for @pixiv_daily")
        .arg(
            Arg::new("chat-id")
                .short('c')
                .long("chat-id")
                .value_name("CHATID")
                .help("chat ID to send photos to")
                .takes_value(true)
                .env("TELOXIDE_CHAT_ID"),
        )
        .arg(
            Arg::new("token")
                .short('t')
                .long("token")
                .value_name("TOKEN")
                .help("Telegram bot token")
                .takes_value(true)
                .env("TELOXIDE_TOKEN"),
        )
        .arg(
            Arg::new("pages")
                .short('p')
                .long("pages")
                .value_name("PAGES")
                .help("number of ranking pages to fetch (18 illusts/page)")
                .takes_value(true)
                .default_value("3")
                .env("PIXIV_RANKING_PAGES"),
        )
        .arg(
            Arg::new("r18")
                .short('r')
                .long("r18")
                .help("run in r18 mode")
                .env("PIXIV_R18"),
        )
        .get_matches();

    // assign command line values to variables
    Ok(Config::new(
        {
            let decorator = slog_term::TermDecorator::new().build();
            let drain = Mutex::new(slog_term::FullFormat::new(decorator).build()).fuse();
            slog::Logger::root(drain, o!())
        },
        matches.value_of_t_or_exit("token"),
        matches.value_of_t_or_exit("chat-id"),
        matches.value_of_t_or_exit("pages"),
        matches.is_present("r18"),
    ))
}

/// program entry point
#[tokio::main]
async fn main() {
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
