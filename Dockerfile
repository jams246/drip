FROM rust:1.99.1-slim-bookworm@sha256:24707e70a4f1e23d9a7b25202ec43126d28a52294b96aad5c670df1fd1f1d400 AS toolchain
WORKDIR /workspace
COPY . .

FROM toolchain AS verify
RUN cargo fmt --all -- --check \
    && cargo clippy -p core -p server --all-targets --all-features --locked -- -D warnings \
    && cargo test -p core -p server --all-features --locked

FROM verify AS build
RUN cargo build --release -p server --locked

FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3 AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends tzdata=2026f-0+deb12u1 \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -m 0700 -o 10001 -g 10001 /var/lib/drip /mirror
COPY --from=build /workspace/target/release/server /usr/local/bin/server
USER 10001:10001
VOLUME ["/var/lib/drip", "/mirror"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/server"]
CMD ["serve", "--listen", "0.0.0.0:8080"]
