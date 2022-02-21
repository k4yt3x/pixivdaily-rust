use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use futures::future;
use image::{imageops::FilterType, ImageError, ImageFormat};
use reqwest::{header, Client};
use serde::Deserialize;
use slog::{debug, error, info, warn};
use teloxide::{
    payloads::PinChatMessageSetters,
    prelude::*,
    types::{InputFile, InputMedia, InputMediaPhoto, ParseMode},
    RequestError,
};
use tokio::{task, task::JoinHandle, time};

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");
const MAX_IMAGE_SIZE: usize = 10 * 1024_usize.pow(2);

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Illust {
    id: String,
    title: String,
    width: String,
    height: String,
    tags: Vec<String>,
    illust_images: Option<Vec<IllustImages>>,
    manga_a: Option<Vec<Manga>>,
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
struct IllustImages {
    illust_image_width: String,
    illust_image_height: String,
}

#[derive(Debug, Deserialize)]
struct Manga {
    page: u32,
    url_big: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct IllustMeta {
    description: String,
    canonical: String,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[derive(Clone)]
pub struct Config {
    logger: slog::Logger,
    token: String,
    chat_id: i64,
}

impl Config {
    pub fn new(logger: slog::Logger, token: String, chat_id: i64) -> Config {
        Config {
            logger,
            token,
            chat_id,
        }
    }
}

/// retrieve and deserialize pixiv daily rankings
///
/// # Errors
///
/// reqwest errors
///
/// # Examples
///
/// ```
/// let contents = get_pixiv_daily_ranking().await?;
/// ```
async fn get_pixiv_daily_ranking() -> Result<Vec<RankingIllust>, reqwest::Error> {
    Ok(
        reqwest::get("https://www.pixiv.net/ranking.php?mode=daily&format=json")
            .await?
            .json::<RankingResponse>()
            .await?
            .contents,
    )
}

/// get detailed information about a specific illustration
///
/// # Arguments
///
/// * `id` - illust ID
///
/// # Errors
///
/// reqwest errors
///
/// # Examples
///
/// ```
/// let illust_details = get_illust_details("87469406").await?;
/// ```
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

/// download an image into memory into Vec<u8>
///
/// # Arguments
///
/// * `url` - URL of the image
/// * `referer` - Referer header to set
///
/// # Errors
///
/// reqwest errors
///
/// # Examples
///
/// ```
/// let image_bytes = download_image(&"https://example.com/example.png",
/// &"https://example.com").await?
/// ```
async fn download_image(url: &String, referer: &String) -> Result<Vec<u8>, reqwest::Error> {
    Ok(reqwest::Client::new()
        .get(url)
        .header(header::REFERER, referer)
        .send()
        .await?
        .bytes()
        .await?
        .to_vec())
}

/// resize an image into a size/dimension acceptable by
/// Telegram's API
///
/// # Arguments
///
/// * `config` - an instance of Config
/// * `image_bytes` - raw input image bytes
/// * `id` - illustration ID
/// * `original_width` - original image width
/// * `original_height` - original image height
///
/// # Errors
///
/// image::ImageError
async fn resize_image(
    config: &Config,
    image_bytes: Vec<u8>,
    id: &String,
    original_width: u32,
    original_height: u32,
) -> Result<Vec<u8>, ImageError> {
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
        "Resizing parameters: r={} w={} h={}", guessed_ratio, target_width, target_height
    );

    // Telegram API requires width + height <= 10000
    if target_width + target_height > 10000 {
        let target_ratio = 10000.0 / (target_width + target_height) as f32;
        target_width = (target_width as f32 * target_ratio).floor() as u32;
        target_height = (target_height as f32 * target_ratio).floor() as u32;
        debug!(
            config.logger,
            "Additional resizing parameters: r={} w={} h={}",
            target_ratio,
            target_width,
            target_height
        );
    }

    // load the image from memory into ImageBuffer
    let mut dynamic_image = image::load_from_memory(&image_bytes)?;

    loop {
        // downsize the image with Lanczos3
        dynamic_image = dynamic_image.resize(target_width, target_height, FilterType::Lanczos3);

        // encode raw bytes into PNG bytes
        let mut png_bytes = vec![];
        dynamic_image.write_to(&mut png_bytes, ImageFormat::Png)?;

        // return the image if it is small enough
        if png_bytes.len() < MAX_IMAGE_SIZE {
            info!(
                config.logger,
                "Final size: size={}MiB",
                png_bytes.len() as f32 / 1024_f32.powf(2.0)
            );
            return Ok(png_bytes);
        }

        // shrink image by another 20% if the previous round is not enough
        debug!(
            config.logger,
            "Image too large: size={}MiB; additional resizing required",
            png_bytes.len() as f32 / 1024_f32.powf(2.0)
        );
        target_width = (target_width as f32 * 0.8) as u32;
        target_height = (target_height as f32 * 0.8) as u32;
    }
}

/// escape characters according to Telegram API's
/// MarkdownV2 specification
///
/// # Arguments
///
/// * `text` - text to escape
///
/// # Examples
///
/// ```
/// let escaped = markdown_escape("This. Is. Sparta!");
/// assert_eq!(escaped, "This\\. Is\\. Sparta\\!".to_owned());
/// ```
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

/// send an illustration to the Telegram chat
///
/// # Arguments
///
/// * `config` - an instance of Config
/// * `bot` - an instance of AutoSend<Bot>
/// * `illust` - an Illust struct which represents an illustration
/// * `send_sleep` - global sleep timer
///
/// # Errors
///
/// any error that implements the Error trait
async fn send_illust<'a>(config: Config, bot: AutoSend<Bot>, illust: Illust) -> Result<()> {
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
    let mut captions = vec![
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

    // holds all InputMedia enums for sendMediaGroup
    let mut images = Vec::new();

    // if illustration is a manga
    if let (Some(manga), Some(illust_images)) = (illust.manga_a, illust.illust_images) {
        // update the caption with the manga's page count
        captions.push(format!("Pages: {}", manga.len()));

        // add each manga into images
        for image in manga {
            info!(
                config.logger,
                "Retrieving manga id={} page={}", illust.id, image.page
            );
            let original_image = download_image(&image.url_big, &illust.meta.canonical).await?;
            let image_bytes = resize_image(
                &config,
                original_image,
                &illust.id,
                illust_images[image.page as usize]
                    .illust_image_width
                    .parse::<u32>()?,
                illust_images[image.page as usize]
                    .illust_image_height
                    .parse::<u32>()?,
            )
            .await?;
            images.push(InputMedia::Photo(InputMediaPhoto {
                media: InputFile::memory("image", image_bytes),
                caption: match images.len() {
                    0 => Some(captions.join("\n")),
                    _ => None,
                },
                parse_mode: match images.len() {
                    0 => Some(ParseMode::MarkdownV2),
                    _ => None,
                },
                caption_entities: None,
            }));

            // one media group can contain a max of 10 images
            if images.len() == 10 {
                break;
            }
        }
    }
    // if this is not a manga
    else {
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
        images.push(InputMedia::Photo(InputMediaPhoto {
            media: InputFile::memory("image", image_bytes),
            caption: Some(captions.join("\n")),
            parse_mode: Some(ParseMode::MarkdownV2),
            caption_entities: None,
        }));
    }

    // contains the final result
    let mut result: Option<Result<Vec<Message>, RequestError>> = None;

    // retry up to 8 times if the API rate limit has been exceeded
    for attempt in 0..10 {
        // send the photo with the caption
        info!(
            config.logger,
            "Sending illustration attempt={} id={}", attempt, illust.id
        );
        result = Some(
            bot.send_media_group(config.chat_id, images.clone())
                .disable_notification(true)
                .await,
        );

        // catch and downcast only if the error is RetryAfter
        if let Some(Err(RequestError::RetryAfter(seconds))) = result {
            warn!(
                config.logger,
                "Hit rate limit: sleeping for {} seconds", seconds
            );
            time::sleep(Duration::from_secs(seconds as u64)).await;
        }
        else if let Some(Err(error)) = &result {
            warn!(
                config.logger,
                "Error sending illustration: id={} message={:?}", illust.id, error
            );
        }
        // break out of the loop if the send operation has succeeded
        else {
            break;
        }
    }

    // return the error if the send operation has not succeeded after 10 attempts
    if let Some(Err(error)) = result {
        error!(
            config.logger,
            "Giving up sending illustration: id={} message={:?}", illust.id, error
        );
        Err(error.into())
    }
    else {
        Ok(())
    }
}

/// entry point for the functional part of this program
///
/// # Arguments
///
/// * `config` - an instance of Config
///
/// # Errors
///
/// any error that implements the Error trait
pub async fn run(config: Config) -> Result<()> {
    info!(
        config.logger,
        "PixivDaily bot {version} initializing",
        version = VERSION
    );

    // initialize bot instance with a custom client
    // the default pool idle timeout is 90 seconds, which is too small
    // for large images to be uploaded
    let client = Client::builder()
        .pool_idle_timeout(Duration::from_secs(6000))
        .build()?;
    let bot = Bot::with_client(&config.token, client).auto_send();

    // fetch daily top 50
    let today = Utc::today().format("%B %-d, %Y").to_string();
    info!(config.logger, "Fetching top 50 illustrations ({})", today);

    // push get illust detail tasks into a Vec
    let mut get_illust_tasks: Vec<JoinHandle<Result<Illust, reqwest::Error>>> = vec![];
    for illust in get_pixiv_daily_ranking().await? {
        get_illust_tasks.push(task::spawn(get_illust_details(
            illust.illust_id.to_string(),
        )));
    }

    // send today's date and pin the message
    let date_message = bot.send_message(config.chat_id, today).await?;
    bot.pin_chat_message(config.chat_id, date_message.id)
        .disable_notification(true)
        .await?;

    // send each of the illustrations
    let mut send_illust_tasks: Vec<JoinHandle<Result<(), anyhow::Error>>> = vec![];
    for illust in future::join_all(get_illust_tasks).await {
        send_illust_tasks.push(task::spawn(send_illust(
            config.clone(),
            bot.clone(),
            illust??,
        )));
    }

    // print errors in finished tasks if any
    for result in future::join_all(send_illust_tasks).await {
        if let Err(error) = result? {
            if let Some(illust_id) = error.downcast_ref::<String>() {
                error!(
                    config.logger,
                    "Error sending photo: {} {}", illust_id, error
                );
            }
            else {
                error!(config.logger, "Error sending photo: {}", error);
            }
        }
    }

    Ok(())
}
