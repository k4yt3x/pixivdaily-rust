# PixivDaily (Rust)

This repository contains the source code of the program running the Telegram channel [@pixiv\_daily](https://t.me/pixiv\_daily).

<p align="center">
   <img src="https://user-images.githubusercontent.com/21986859/130876907-80e3416a-01bc-446d-a56f-33a6198b8ff0.png"/>
</p>

## Usage

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

Or, you can run it with systemd. The default systemd timer runs the program at every midnight. Remember to update the fields in `/etc/pixivdaily.conf`.

```shell
sudo cp target/release/pixivdaily /usr/local/bin/pixivdaily
sudo cp conf/pixivdaily.service conf/pixivdaily.timer /etc/systemd/system
sudo cp conf/pixivdaily.conf /etc/pixivdaily.conf
sudo systemctl daemon-reload
sudo systemctl enable --now pixivdaily.timer
```
