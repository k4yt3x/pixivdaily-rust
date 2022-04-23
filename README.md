# PixivDaily (Rust)

This repository contains the source code of the backend program running the Telegram channels [@pixiv_daily](https://t.me/pixiv_daily) and [@pixiv_daily_r18](https://t.me/pixiv_daily_r18).

<p align="center">
   <img src="https://user-images.githubusercontent.com/21986859/164949671-5a8cf248-3feb-409e-91f9-9854b2038bce.png"/>
</p>

## Run in a Container

You will obviously first have to have an OCI-compatible container runtime like Podman or Docker installed. Then, pull and run the container:

```shell
sudo podman run -e TELOXIDE_TOKEN=$TELOXIDE_TOKEN -e TELOXIDE_CHAT_ID=$TELOXIDE_CHAT_ID ghcr.io/k4yt3x/pixivdaily:1.4.0
```

You can pass the settings either through environment variables or arguments. For details, see the help page of the binary:

```shell
sudo podman run ghcr.io/k4yt3x/pixivdaily:1.4.0 -h
```

## Run From Source

First, you'll need to clone and build this program. For this step, you will need `cargo` to be installed and the `rustc` compiler available.

```shell
git clone https://github.com/k4yt3x/pixivdaily-rust
cd pixivdaily-rust
cargo build --release
```

After the binary is built, you can either run it directly:

```shell
./target/release/pixivdaily -c [CHAT_ID] -t [TOKEN]
```

...or run it with systemd. The default systemd timer runs the program at every midnight. Remember to update the fields in `/etc/pixivdaily.conf`.

```shell
sudo cp target/release/pixivdaily /usr/local/bin/pixivdaily
sudo cp conf/pixivdaily.service conf/pixivdaily.timer /etc/systemd/system
sudo cp conf/pixivdaily.conf /etc/pixivdaily.conf
sudo systemctl daemon-reload
sudo systemctl enable --now pixivdaily.timer
```
