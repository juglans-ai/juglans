FROM alpine:3.21 AS builder
ARG TARGETARCH
RUN apk add --no-cache curl tar
RUN case "$TARGETARCH" in \
      amd64) ARCH="x86_64" ;; \
      arm64) ARCH="aarch64" ;; \
      *) echo "Unsupported arch: $TARGETARCH" && exit 1 ;; \
    esac && \
    curl -sSL "https://github.com/rust-lang/mdBook/releases/download/v0.4.43/mdbook-v0.4.43-${ARCH}-unknown-linux-musl.tar.gz" \
    | tar xz -C /usr/local/bin
WORKDIR /book
COPY . .
RUN mdbook build --dest-dir /out

FROM nginx:alpine
COPY --from=builder /out /usr/share/nginx/html
EXPOSE 80
