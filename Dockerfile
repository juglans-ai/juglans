FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates python3 \
    && rm -rf /var/lib/apt/lists/*

COPY juglans /usr/local/bin/
COPY workers/ /usr/local/bin/workers/

WORKDIR /workspace
EXPOSE 8080

CMD ["juglans", "web", "--host", "0.0.0.0", "--port", "8080"]
