FROM rust:slim-buster AS rust

RUN apt-get update -y \
 && apt-get install -y --no-install-recommends ca-certificates libgit2-dev libssl-dev pkgconf

RUN mkdir /build/
WORKDIR /build/
COPY . .
RUN cargo build --release --locked \
 && mv "target/release/stickerpicker" . \
 && strip stickerpicker

########################################################################################################################

FROM debian:buster-slim

LABEL org.opencontainer.image.url="https://github.com/users/msrd0/packages/container/package/stickerpicker"
LABEL org.opencontainer.image.title="Stickerpicker for Element"
LABEL org.opencontainer.image.source="https://github.com/msrd0/docker-stickerpicker"

RUN apt-get update -y \
 && apt-get install -y --no-install-recommends ca-certificates libgit2-27 \
 && apt-get clean \
 && rm -rf /var/lib/apt/lists/*

ENV RUST_LOG=info
COPY --from=rust /build/stickerpicker /usr/local/bin/stickerpicker
EXPOSE 8080
CMD ["/usr/local/bin/stickerpicker"]
