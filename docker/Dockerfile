# Build a fully static (musl) magicjar and ship it on an empty base image.
#
# Alpine's toolchain is musl-based, so a release build links statically with no
# runtime dependencies. buildx compiles the `build` stage once per --platform,
# so this single file yields both linux/amd64 and linux/arm64 artifacts.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev binutils
WORKDIR /src
COPY . .
RUN cargo build --release --locked \
    && strip target/release/magicjar \
    && cp target/release/magicjar /magicjar

# Export-only stage. `buildx build --target bin --output type=local` writes just
# the binary, which the release workflow attaches to the GitHub Release.
FROM scratch AS bin
COPY --from=build /magicjar /magicjar

# Default runtime image, intended for `COPY --from`:
#   COPY --from=ghcr.io/clintval/magicjar:0.1.0 /magicjar /usr/local/bin/magicjar
FROM scratch
COPY --from=build /magicjar /magicjar
ENTRYPOINT ["/magicjar"]
