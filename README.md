# 游隼 (Peregrine) - 高性能代理服务器

游隼是一个用 Rust 编写的高性能、轻量级代理服务器，支持 HTTP/HTTPS 和 SOCKS5 协议，具有访问控制、多级代理链和 Prometheus 监控等功能。

## 特性

- **HTTP 代理**: 正向代理 + CONNECT 隧道支持
- **HTTPS 支持**: TLS 监听器和上游 TLS 连接
- **SOCKS5 代理**: TCP CONNECT 和 UDP ASSOCIATE（RFC 1928/1929）
- **访问控制**: 
  - IP CIDR 过滤
  - 域名/URL 过滤（支持 Glob 和正则表达式）
  - HTTP 方法过滤
  - Basic Auth 认证
- **上游代理**: 直连、HTTP 代理、SOCKS5 代理、多级代理链
- **Prometheus 监控**: `/metrics` 端点提供连接数、请求数、延迟、错误统计
- **热重载配置**: SIGHUP 信号触发配置重载
- **优雅关闭**: 连接排空，30秒超时
- **结构化日志**: 支持 JSON 和文本格式日志

## 快速开始

### 安装

#### 从源码构建

```bash
git clone https://github.com/baiyulong/PeregrineProxy.git
cd PeregrineProxy
cargo build --release
```

二进制文件位于 `target/release/peregrine`。

#### 使用 Docker

```bash
docker build -t peregrine:latest .
docker run -d \
  --name peregrine \
  -p 8080:8080 \
  -p 1080:1080 \
  -p 9090:9090 \
  -v /path/to/config.yaml:/etc/peregrine/config.yaml \
  peregrine:latest
```

### 运行

```bash
./peregrine --config config.yaml
```

#### 命令行选项

```
选项：
  -c, --config <FILE>    配置文件路径
  -h, --help             显示帮助信息
  -V, --version          显示版本信息
```

## 配置

### 配置文件格式

配置文件采用 YAML 格式。关键部分：

#### 服务器配置

```yaml
server:
  listen:
    - addr: "0.0.0.0:8080"
      protocol: http
    - addr: "0.0.0.0:1080"
      protocol: socks5
  max_connections: 10000
  tcp_keepalive: 60
  connect_timeout: 10
  read_timeout: 30
  write_timeout: 30
```

#### 访问控制

```yaml
access_control:
  default_action: deny  # allow, deny, authenticate
  rules:
    - action: allow
      src_ip: ["127.0.0.1/32", "192.168.0.0/16"]
    - action: deny
      dst_domain: ["*.blocked.com"]
    - action: authenticate
      auth:
        type: basic
        users:
          - username: admin
            password: secret123
```

#### 上游代理

```yaml
upstream:
  # 直连
  type: direct

  # HTTP 上游代理
  # type: http
  # addr: "proxy.example.com:3128"
  # auth:
  #   username: user
  #   password: pass

  # SOCKS5 上游代理
  # type: socks5
  # addr: "socks-proxy.example.com:1080"

  # 代理链
  # type: chain
  # chain:
  #   - type: http
  #     addr: "first-proxy.com:3128"
  #   - type: socks5
  #     addr: "second-proxy.com:1080"
```

#### 日志配置

```yaml
logging:
  level: info              # trace, debug, info, warn, error
  format: json             # json 或 text
  access_log: "./access.log"
```

#### 监控配置

```yaml
metrics:
  enabled: true
  listen: "0.0.0.0:9090"
```

更多配置示例，请参考 `config.example.yaml`。

## Docker 使用

### 构建镜像

```bash
docker build -t peregrine:latest .
```

### 运行容器

使用默认配置：

```bash
docker run -d \
  --name peregrine \
  -p 8080:8080 \
  -p 1080:1080 \
  -p 9090:9090 \
  peregrine:latest
```

使用自定义配置：

```bash
docker run -d \
  --name peregrine \
  -p 8080:8080 \
  -p 1080:1080 \
  -p 9090:9090 \
  -v $(pwd)/config.yaml:/etc/peregrine/config.yaml:ro \
  peregrine:latest
```

### Docker Compose

创建 `docker-compose.yml`：

```yaml
version: '3.8'
services:
  peregrine:
    build: .
    ports:
      - "8080:8080"
      - "1080:1080"
      - "9090:9090"
    volumes:
      - ./config.yaml:/etc/peregrine/config.yaml:ro
    restart: unless-stopped
```

运行：

```bash
docker-compose up -d
```

## 使用示例

### HTTP 代理

使用 curl 通过 HTTP 代理访问：

```bash
curl -x http://localhost:8080 https://example.com
```

使用代理进行 CONNECT 隧道（HTTPS）：

```bash
curl -p http://localhost:8080 https://example.com
```

### SOCKS5 代理

使用 curl 通过 SOCKS5 代理访问：

```bash
curl -x socks5://localhost:1080 https://example.com
```

### 带认证的代理

```bash
curl -x http://user:pass@localhost:8080 https://example.com
curl -x socks5://user:pass@localhost:1080 https://example.com
```

## 监控

### Prometheus 指标

访问 `/metrics` 端点获取 Prometheus 格式的指标：

```bash
curl http://localhost:9090/metrics
```

提供的关键指标：

- `peregrine_connections`: 当前活跃连接数
- `peregrine_requests_total`: 总请求数
- `peregrine_request_duration_seconds`: 请求延迟分布
- `peregrine_errors_total`: 错误总数
- `peregrine_upstream_type`: 上游代理类型

### 热重载配置

修改配置文件后，发送 SIGHUP 信号重载配置（无需重启）：

```bash
kill -SIGHUP <pid>
```

## 许可证

本项目采用 MIT 许可证。详见 [LICENSE](LICENSE) 文件。

## 贡献

欢迎提交 Issue 和 Pull Request！

## 致谢

感谢所有的贡献者和使用者。
