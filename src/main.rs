use std::{process, sync::Mutex};

use clap::{value_t_or_exit, Arg};
use pixivdaily::{run, Config};
use slog::{o, Drain};

/// parse command line arguments into a Config struct
///
/// # Errors
///
/// anything that implements the Error trait
fn parse() -> Option<Config> {
    // parse command line arguments
    let matches = clap::App::new("pixivdaily")
        .version("1.0.0")
        .author("K4YT3X <i@k4yt3x.com>")
        .about("Source code for the Telegram channel @pixivdaily")
        .arg(
            Arg::with_name("chat-id")
                .short("c")
                .long("chat-id")
                .value_name("CHATID")
                .help("chat ID to send photos to")
                .default_value("-1001181003645")
                .takes_value(true),
        )
        .get_matches();

    // assign command line values to variables
    Config::new(
        {
            let decorator = slog_term::TermDecorator::new().build();
            let drain = Mutex::new(slog_term::FullFormat::new(decorator).build()).fuse();
            slog::Logger::root(drain, o!())
        },
        value_t_or_exit!(matches.value_of("chat-id"), i64),
    )
}

/// program entry point
#[tokio::main]
async fn main() {
    // parse command line arguments into Config
    if let Some(config) = parse() {
        // run ping with config
        process::exit(match run(config).await {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {}", e);
                1
            }
        })
    }
    else {
        process::exit(1);
    }
}
