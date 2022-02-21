use std::{process, sync::Mutex};

use anyhow::Result;
use clap::{value_t_or_exit, Arg};
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
    let matches = clap::App::new("pixivdaily")
        .version(VERSION)
        .author("K4YT3X <i@k4yt3x.com>")
        .about("A Telegram bot that posts Pixiv's daily rankings for @pixiv_daily")
        .arg(
            Arg::with_name("chat-id")
                .short("c")
                .long("chat-id")
                .value_name("CHATID")
                .help("chat ID to send photos to")
                .takes_value(true)
                .env("TELOXIDE_CHAT_ID"),
        )
        .arg(
            Arg::with_name("token")
                .short("t")
                .long("token")
                .value_name("TOKEN")
                .help("Telegram bot token")
                .takes_value(true)
                .env("TELOXIDE_TOKEN"),
        )
        .get_matches();

    // assign command line values to variables
    Ok(Config::new(
        {
            let decorator = slog_term::TermDecorator::new().build();
            let drain = Mutex::new(slog_term::FullFormat::new(decorator).build()).fuse();
            slog::Logger::root(drain, o!())
        },
        value_t_or_exit!(matches.value_of("token"), String),
        value_t_or_exit!(matches.value_of("chat-id"), i64),
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
