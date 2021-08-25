use std::{env, error::Error, process, sync::Mutex};

use clap::{value_t_or_exit, Arg};
use pixivdaily::{run, Config};
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
fn parse() -> Result<Config, Box<dyn Error>> {
    // parse command line arguments
    let matches = clap::App::new("pixivdaily")
        .version("1.0.0")
        .author("K4YT3X <i@k4yt3x.com>")
        .about("Source code for the Telegram channel @pixiv_daily")
        .arg(
            Arg::with_name("chat-id")
                .short("c")
                .long("chat-id")
                .value_name("CHATID")
                .help("chat ID to send photos to")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("token")
                .short("t")
                .long("token")
                .value_name("TOKEN")
                .help("Telegram bot token")
                .takes_value(true),
        )
        .get_matches();

    let chat_id = env::var("TELOXIDE_CHAT_ID")
        .unwrap_or_else(|_| value_t_or_exit!(matches.value_of("chat-id"), String));

    let token = env::var("TELOXIDE_TOKEN")
        .unwrap_or_else(|_| value_t_or_exit!(matches.value_of("token"), String));

    // assign command line values to variables
    Ok(Config::new(
        {
            let decorator = slog_term::TermDecorator::new().build();
            let drain = Mutex::new(slog_term::FullFormat::new(decorator).build()).fuse();
            slog::Logger::root(drain, o!())
        },
        token,
        chat_id.parse::<i64>()?,
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
