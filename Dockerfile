FROM rust:1.66-alpine3.17 as builder
COPY . /app
WORKDIR /app
RUN apk add --no-cache --virtual .build-deps \
        make \
        musl-dev \
        openssl-dev \
        perl \
        pkgconfig \
    && cargo build --release --target x86_64-unknown-linux-musl

FROM gcr.io/distroless/static:nonroot
LABEL maintainer="K4YT3X <i@k4yt3x.com>" \
      org.opencontainers.image.source="https://github.com/k4yt3x/pixivdaily-rust" \
      org.opencontainers.image.description="The backend of the Telegram channel @pixiv_daily"
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/pixivdaily \
                    /usr/local/bin/pixivdaily
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/pixivdaily"]
