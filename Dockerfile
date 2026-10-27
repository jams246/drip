FROM rust:1.99.1-slim-bookworm@sha256:86234e857a413004348b471d11a4ebe1aa92d2eae2b6b143cac68fd1ac3c48be AS toolchain
WORKDIR /workspace
COPY . .

FROM toolchain AS verify
RUN cargo fmt --all -- --check \
    && cargo clippy -p core -p server --all-targets --all-features --locked -- -D warnings \
    && cargo test -p core -p server --all-features --locked

FROM verify AS build
RUN cargo build --release -p server --locked

FROM debian:bookworm-slim@sha256:ec31d744f83f4a73447ba045aaab9409997efe5cc706eca4890c1be36cc1606a AS runtime
RUN mkdir -p /var/lib/drip /mirror && chown -R 10001:10001 /var/lib/drip /mirror
COPY --from=build /workspace/target/release/server /usr/local/bin/server
USER 10001:10001
VOLUME ["/var/lib/drip", "/mirror"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/server"]
CMD ["serve"]
