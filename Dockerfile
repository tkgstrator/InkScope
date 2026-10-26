FROM rust:trixie AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --workspace

FROM gcr.io/distroless/cc-debian13:nonroot AS runtime
COPY --from=build /app/target/release/inkscope /usr/local/bin/inkscope
ENTRYPOINT ["/usr/local/bin/inkscope"]
