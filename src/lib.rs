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
use std::io::{Cursor, Read, Seek, SeekFrom};

use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::future;
use image::{imageops::FilterType, ImageError, ImageFormat};
use reqwest::header;
use serde::Deserialize;
use teloxide::{
    adaptors::throttle::{Limits, Throttle},
    payloads::PinChatMessageSetters,
    prelude::*,
    types::{ChatId, InputFile, InputMedia, InputMediaPhoto, ParseMode},
    RequestError,
};
use tokio::{task, task::JoinHandle};
use tracing::{debug, error, info, warn};

pub const VERSION: &'static str = env!("CARGO_PKG_VERSION");
const MAX_IMAGE_SIZE: usize = 10 * 1024_usize.pow(2);
const USER_AGENT: &'static str = "PixivAndroidApp/6.135.1 (Android 15; Pixel 9)";

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
    url: Option<String>,
    url_s: Option<String>,
    url_ss: Option<String>,
    meta: IllustMeta,
    author_details: Author,
    is_login_only: bool,
}

#[derive(Debug, Deserialize)]
struct IllustImages {
    illust_image_width: String,
    illust_image_height: String,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Manga {
    page: u32,
    url: String,
    url_small: String,
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
    body: RankingBody,
}

#[derive(Debug, Deserialize)]
struct RankingBody {
    ranking: Vec<RankingIllust>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RankingIllust {
    illust_id: String,
    rank: u32,
}

/// configs passed to the run function
#[derive(Clone)]
pub struct Config {
    token: String,
    chat_id: ChatId,
    pages: u32,
    r18: bool,
}

impl Config {
    pub fn new(token: String, chat_id: i64, pages: u32, r18: bool) -> Config {
        Config {
            token,
            chat_id: ChatId(chat_id),
            pages,
            r18,
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
/// let ranking = get_pixiv_daily_ranking(&config).await?;
/// ```
async fn get_pixiv_daily_ranking(config: &Config) -> Result<Vec<RankingIllust>, reqwest::Error> {
    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let mut illusts = Vec::new();

    for page in 1..config.pages + 1 {
        illusts.push(
            client
                .get(format!(
                    "https://www.pixiv.net/touch/ajax/ranking/illust?mode={}&type=all&page={}",
                    {
                        if config.r18 {
                            "daily_r18"
                        }
                        else {
                            "daily"
                        }
                    },
                    page
                ))
                .send()
                .await?
                .json::<RankingResponse>()
                .await?
                .body
                .ranking,
        )
    }

    // flatten Vec<Vec<RankingIllust>> and return
    Ok(illusts.into_iter().flatten().collect())
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
    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    let illust_response = client
        .get(format!(
            "https://www.pixiv.net/touch/ajax/illust/details?illust_id={}",
            id
        ))
        .send()
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
    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;
    Ok(client
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
    image_bytes: Vec<u8>,
    id: &String,
    original_width: u32,
    original_height: u32,
) -> Result<Vec<u8>, ImageError> {
    // if image is already small enough, return original image
    if image_bytes.len() <= MAX_IMAGE_SIZE {
        return Ok(image_bytes);
    }
    info!(id = %id, "Resizing oversized image");

    // this is a very rough guess
    // could be improved in the future
    let guessed_ratio = (MAX_IMAGE_SIZE as f32 / image_bytes.len() as f32).sqrt();
    let mut target_width = (original_width as f32 * guessed_ratio) as u32;
    let mut target_height = (original_height as f32 * guessed_ratio) as u32;
    debug!(
        r = guessed_ratio,
        w = target_width,
        h = target_height,
        "Resizing parameters"
    );

    // Telegram API requires width + height <= 10000
    if target_width + target_height > 10000 {
        let target_ratio = 10000.0 / (target_width + target_height) as f32;
        target_width = (target_width as f32 * target_ratio).floor() as u32;
        target_height = (target_height as f32 * target_ratio).floor() as u32;
        debug!(
            target_ratio = target_ratio,
            target_width = target_width,
            target_height = target_height,
            "Additional resizing parameters"
        );
    }

    // load the image from memory into ImageBuffer
    let mut dynamic_image = image::load_from_memory(&image_bytes)?;

    loop {
        // downsize the image with Lanczos3
        dynamic_image = dynamic_image.resize(target_width, target_height, FilterType::Lanczos3);

        // encode raw bytes into PNG bytes
        let mut png_bytes_cursor = Cursor::new(vec![]);
        dynamic_image.write_to(&mut png_bytes_cursor, ImageFormat::Png)?;
        png_bytes_cursor.seek(SeekFrom::Start(0))?;

        // read all bytes from cursor
        let mut png_bytes = Vec::new();
        png_bytes_cursor.read_to_end(&mut png_bytes)?;

        // return the image if it is small enough
        if png_bytes.len() < MAX_IMAGE_SIZE {
            info!(
                size_mib = png_bytes.len() as f32 / 1024_f32.powf(2.0),
                "Final size"
            );
            return Ok(png_bytes);
        }

        // shrink image by another 20% if the previous round is not enough
        debug!(
            size_mib = png_bytes.len() as f32 / 1024_f32.powf(2.0),
            "Image too large; additional resizing required"
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
/// * `bot` - an instance of Throttle<Bot>
/// * `illust` - an Illust struct which represents an illustration
/// * `send_sleep` - global sleep timer
///
/// # Errors
///
/// any error that implements the Error trait
async fn send_illust<'a>(config: Config, bot: Throttle<Bot>, illust: Illust) -> Result<()> {
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
                id = %illust.id,
                page = image.page,
                "Retrieving manga"
            );
            let original_image = download_image(&image.url, &illust.meta.canonical).await?;
            let image_bytes = resize_image(
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
                media: InputFile::memory(image_bytes),
                caption: match images.len() {
                    0 => Some(captions.join("\n")),
                    _ => None,
                },
                parse_mode: match images.len() {
                    0 => Some(ParseMode::MarkdownV2),
                    _ => None,
                },
                caption_entities: None,
                has_spoiler: false,
            }));

            // one media group can contain a max of 10 images
            if images.len() == 10 {
                break;
            }
        }
    }
    // if this is not a manga
    else {
        info!(id = %illust.id, "Retrieving image");

        // An URL might not be available for illustrations that are login-only
        let url: String = match illust.url {
            Some(url) => url,
            None => {
                return Err(anyhow!("Illustration URL is not available"));
            }
        };

        let original_image = download_image(&url, &illust.meta.canonical).await?;
        let image_bytes = resize_image(
            original_image,
            &illust.id,
            illust.width.parse::<u32>()?,
            illust.height.parse::<u32>()?,
        )
        .await?;
        images.push(InputMedia::Photo(InputMediaPhoto {
            media: InputFile::memory(image_bytes),
            caption: Some(captions.join("\n")),
            parse_mode: Some(ParseMode::MarkdownV2),
            caption_entities: None,
            has_spoiler: false,
        }));
    }

    // contains the final result
    let mut result: Option<Result<Vec<Message>, RequestError>> = None;

    // retry up to 10 times since the send attempt might run into temporary errors like
    // Api(Unknown("Bad Request: group send failed"))
    for attempt in 0..10 {
        // send the photo with the caption
        info!(
            id = %illust.id,
            attempt = attempt,
            "Sending artwork"
        );
        result = Some(
            bot.send_media_group(config.chat_id, images.clone())
                .disable_notification(true)
                .await,
        );

        // if an error has occurred, print the error's message
        if let Some(Err(error)) = &result {
            warn!(
                id = %illust.id,
                error = ?error,
                "Temporary error sending artwork"
            );
        }
        // break out of the loop if the send operation has succeeded
        else {
            debug!(
                id = %illust.id,
                attempt = attempt,
                "Successfully sent artwork"
            );
            break;
        }
    }

    // return the error if the send operation has not succeeded after 10 attempts
    if let Some(Err(error)) = result {
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
    info!(version = VERSION, "PixivDaily bot initializing");

    // initialize bot instance
    let bot = Bot::new(&config.token).throttle(Limits::default());

    // fetch daily top 50
    let today = Utc::now().format("%B %-d, %Y").to_string();
    info!(
        date = today,
        pages = config.pages,
        r18 = config.r18,
        "Fetching illustrations",
    );

    // push get illust detail tasks into a Vec
    let mut get_illust_tasks: Vec<JoinHandle<Result<Illust, reqwest::Error>>> = vec![];
    for illust in get_pixiv_daily_ranking(&config).await? {
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
                error!(id = illust_id, error = ?error, "Failed sending artwork");
            }
            else {
                error!(error = ?error, "Failed sending artwork: message=");
            }
        }
    }

    Ok(())
}
