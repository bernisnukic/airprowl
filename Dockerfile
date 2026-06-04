# airprowl — multi-stage build (Linux x86_64).
#
#   docker build -t airprowl .
#   docker run --rm -it airprowl --demo            # simulated TUI, no hardware
#
# Real scanning needs host hardware access and only works on a Linux host:
#   docker run --rm -it --net=host --privileged \
#     -v /var/run/dbus:/var/run/dbus airprowl

FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
    libdbus-1-dev libusb-1.0-0-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libdbus-1-3 libusb-1.0-0 network-manager iw bluez ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/airprowl /usr/local/bin/airprowl
ENTRYPOINT ["airprowl"]
