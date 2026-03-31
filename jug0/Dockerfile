# =======================
# Stage 1: 构建阶段
# =======================
FROM rust:1.90-bookworm AS builder

WORKDIR /usr/src/jug0

# 1. 复制 Cargo 配置文件，利用 Docker 缓存机制
# 先复制依赖描述文件，创建假的 src 进行编译，这样可以缓存依赖库的编译结果
COPY Cargo.toml Cargo.lock ./
COPY db/Cargo.toml db/Cargo.toml
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && mkdir -p db/src && echo "fn main() {}" > db/src/lib.rs && echo "fn main() {}" > db/src/main.rs
RUN cargo build --release && cargo build --release -p migration

# 2. 复制真正的源代码
COPY . .
# 更新 mtime 确保 cargo 重新编译 (因为刚才创建了假的 main.rs)
RUN touch src/main.rs db/src/main.rs db/src/lib.rs

# 3. 编译真正的二进制文件
RUN cargo build --release && cargo build --release -p migration

# =======================
# Stage 2: 运行阶段 (极简)
# =======================
FROM debian:bookworm-slim

# 安装 SSL 证书 (HTTPS 请求必须) 和必要的系统库
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 创建 uploads 目录，确保持久化挂载点存在
RUN mkdir -p /app/uploads

# 从构建阶段复制编译好的二进制文件
# 注意：target/release/ 下的文件名取决于 Cargo.toml 中的 [package] name
COPY --from=builder /usr/src/jug0/target/release/jug0 .
COPY --from=builder /usr/src/jug0/target/release/migration .

# 如果有 .env 模板或 config 文件，也可以复制
# COPY .env.example .

EXPOSE 3000

CMD ["./jug0"]