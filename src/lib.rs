use std::error::Error;

use futures::future::join_all;
use image::{imageops::FilterType::Lanczos3, load_from_memory, ImageError, ImageFormat::Png};
use reqwest::header::REFERER;
use serde::Deserialize;
use slog::{debug, error, info};
use teloxide::{
    payloads::SendPhotoSetters,
    prelude::*,
    types::{InputFile, ParseMode::MarkdownV2},
};
use tokio::{spawn, task::JoinHandle};

const MAX_IMAGE_SIZE: usize = 10 * 1024_usize.pow(2);

#[derive(Debug, Deserialize)]
struct IllustResponse {
    error: bool,
    message: String,
    body: IllustBody,
}

#[derive(Debug, Deserialize)]
struct IllustBody {
    illust_details: Illust,
}

#[derive(Debug, Deserialize)]
struct Illust {
    id: String,
    title: String,
    width: String,
    height: String,
    tags: Vec<String>,
    rating_count: String,
    rating_view: String,
    bookmark_user_total: u32,
    url_s: String,
    url_ss: String,
    url_big: String,
    meta: IllustMeta,
    author_details: Author,
}

#[derive(Debug, Deserialize)]
struct IllustMeta {
    description: String,
    canonical: String,
}

#[derive(Debug, Deserialize)]
struct Author {
    user_id: String,
    user_name: String,
    user_account: String,
}

#[derive(Debug, Deserialize)]
struct RankingResponse {
    contents: Vec<RankingIllust>,
}

#[derive(Debug, Deserialize)]
struct RankingIllust {
    title: String,
    tags: Vec<String>,
    illust_id: u32,
    rank: u32,
    rating_count: u32,
    view_count: u32,
}

/// configs passed to the run function
pub struct Config {
    logger: slog::Logger,
    chat_id: i64,
}

impl Config {
    pub fn new(logger: slog::Logger, chat_id: i64) -> Option<Config> {
        Some(Config {
            logger,
            chat_id,
        })
    }
}

async fn get_pixiv_daily_ranking() -> Result<Vec<RankingIllust>, Box<dyn Error>> {
    Ok(
        reqwest::get("https://www.pixiv.net/ranking.php?mode=daily&format=json")
            .await?
            .json::<RankingResponse>()
            .await?
            .contents,
    )
}

async fn get_illust_details(id: String) -> Result<Illust, reqwest::Error> {
    let illust_response = reqwest::get(format!(
        "https://www.pixiv.net/touch/ajax/illust/details?illust_id={}",
        id
    ))
    .await?
    .json::<IllustResponse>()
    .await?;

    Ok(illust_response.body.illust_details)
}

async fn download_image(url: &String, referer: &String) -> Result<Vec<u8>, reqwest::Error> {
    Ok(reqwest::Client::new()
        .get(url)
        .header(REFERER, referer)
        .send()
        .await?
        .bytes()
        .await?
        .to_vec())
}

async fn resize_image(
    config: &Config,
    image_bytes: Vec<u8>,
    id: &String,
    original_width: u32,
    original_height: u32,
) -> Result<Vec<u8>, ImageError> {
    debug!(
        config.logger,
        "id={},size={}",
        id,
        image_bytes.len() as f32 / 1024_f32.powf(2.0)
    );

    // if image is already small enough, return original image
    if image_bytes.len() <= MAX_IMAGE_SIZE {
        return Ok(image_bytes);
    }
    info!(config.logger, "Resizing oversized image id={}", id);

    // this is a very rough guess
    // could be improved in the future
    let guessed_ratio = (MAX_IMAGE_SIZE as f32 / image_bytes.len() as f32).sqrt();
    let mut target_width = (original_width as f32 * guessed_ratio) as u32;
    let mut target_height = (original_height as f32 * guessed_ratio) as u32;
    debug!(
        config.logger,
        "Resizing parameters: r={},w={},h={}", guessed_ratio, target_width, target_height
    );

    // Telegram API requires width + height <= 10000
    if target_width + target_height > 10000 {
        let target_ratio = 10000.0 / (target_width + target_height) as f32;
        target_width = (target_width as f32 * target_ratio).floor() as u32;
        target_height = (target_height as f32 * target_ratio).floor() as u32;
        debug!(
            config.logger,
            "Additional resizing parameters: r={},w={},h={}",
            target_ratio,
            target_width,
            target_height
        );
    }

    // load the image from memory into ImageBuffer
    let mut dynamic_image = load_from_memory(&image_bytes)?;

    loop {
        // downsize the image with Lanczos3
        dynamic_image = dynamic_image.resize(target_width, target_height, Lanczos3);

        // encode raw bytes into PNG bytes
        let mut png_bytes = vec![];
        dynamic_image.write_to(&mut png_bytes, Png)?;

        // return the image if it is small enough
        if png_bytes.len() < MAX_IMAGE_SIZE {
            info!(
                config.logger,
                "Final size: {} MiB",
                png_bytes.len() as f32 / 1024_f32.powf(2.0)
            );
            return Ok(png_bytes);
        }

        // shrink image by another 20% if the previous round is not enough
        debug!(
            config.logger,
            "Image too large ({} MiB); additional resizing required",
            png_bytes.len() as f32 / 1024_f32.powf(2.0)
        );
        target_width = (target_width as f32 * 0.8) as u32;
        target_height = (target_height as f32 * 0.8) as u32;
    }
}

fn markdown_escape(text: &String) -> String {
    text.replace("_", "\\_")
        .replace("*", "\\*")
        .replace("[", "\\[")
        .replace("]", "\\]")
        .replace("(", "\\(")
        .replace(")", "\\)")
        .replace("~", "\\~")
        .replace("`", "\\`")
        .replace(">", "\\>")
        .replace("#", "\\#")
        .replace("+", "\\+")
        .replace("-", "\\-")
        .replace("=", "\\=")
        .replace("|", "\\|")
        .replace("{", "\\{")
        .replace("}", "\\}")
        .replace(".", "\\.")
        .replace("!", "\\!")
}

async fn send_illust(
    config: &Config,
    bot: &AutoSend<Bot>,
    illust: &Illust,
    silent: bool,
) -> Result<(), Box<dyn Error>> {
    info!(config.logger, "Retrieving image id={}", illust.id);
    let original_image = download_image(&illust.url_big, &illust.meta.canonical).await?;

    let image_bytes = resize_image(
        &config,
        original_image,
        &illust.id,
        illust.width.parse::<u32>()?,
        illust.height.parse::<u32>()?,
    )
    .await?;

    // download image into memoery
    // and convert it into an InputFile
    let image = InputFile::memory("image", image_bytes);

    let mut tag_strings = vec![];

    for tag in &illust.tags {
        tag_strings.push(
            format!(
                "[\\#{}](https://www\\.pixiv\\.net/tags/{}/artworks)",
                markdown_escape(tag),
                markdown_escape(tag)
            )
            .to_owned(),
        );
    }

    // format captions
    // each element is one line
    let captions = vec![
        format!(
            "Title: [{} \\({}\\)](https://www\\.pixiv\\.net/artworks/{})",
            markdown_escape(&illust.title),
            illust.id,
            illust.id
        )
        .to_owned(),
        format!(
            "Author: [{}](https://www\\.pixiv\\.net/users/{})",
            markdown_escape(&illust.author_details.user_name),
            illust.author_details.user_id
        )
        .to_owned(),
        format!("Tags: {}", tag_strings.join(", ")),
    ];

    // send the photo with the caption
    info!(config.logger, "Sending illustration id={}", illust.id);
    bot.send_photo(config.chat_id, image)
        .parse_mode(MarkdownV2)
        .caption(captions.join("\n"))
        .disable_notification(silent)
        .await?;

    Ok(())
}

pub async fn run(config: Config) -> Result<(), Box<dyn Error>> {
    let bot = Bot::from_env().auto_send();
    let mut first_message = true;

    let mut tasks: Vec<JoinHandle<Result<Illust, reqwest::Error>>> = vec![];

    for illust in get_pixiv_daily_ranking().await? {
        tasks.push(spawn(get_illust_details(illust.illust_id.to_string())));
    }

    for task in join_all(tasks).await {
        let illust = task??;

        for attempt in 0..3 {
            match send_illust(&config, &bot, &illust, !first_message).await {
                Ok(_) => {
                    first_message = false;
                    break;
                }
                Err(e) => {
                    error!(
                        config.logger,
                        "id={},attempt={}: {}", &illust.id, attempt, e
                    );
                }
            }
        }

        /*
        send_illust(&config, &bot, &illust, !first_message)
            .await
            .unwrap_or_else(|error| error!(config.logger, "id={}: {}", &illust.id, error))
        */
    }

    Ok(())
}
