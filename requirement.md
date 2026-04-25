
---

# 游隼（Peregrine）代理软件 – 开发设计文档

| 文档版本 | 1.0 |
| :--- | :--- |
| 编写日期 | 2026-04-25 |
| 项目代号 | Peregrine |
| 中文名称 | 游隼 |
| 技术栈 | Rust (Tokio, Hyper, Rustls) |
| 目标平台 | Windows / Linux / macOS |

> “游隼”是世界上飞行速度最快的动物（俯冲时速超过 300 km/h），以此命名，象征本代理软件 **轻量、极速、精准** 的核心追求。

---

## 1. 项目概述

### 1.1 项目定位
一款高性能、安全的轻量级代理服务器，支持 HTTP/HTTPS 与 SOCKS5 协议，提供访问控制及多级代理链能力。**不包含 Web 缓存功能**，专注于转发效率与协议兼容性。

### 1.2 设计目标
- 纯异步 I/O，高并发低延迟
- 支持 HTTP 代理（含 CONNECT 隧道）与 SOCKS5 代理（TCP + UDP）
- 灵活的上游代理链（HTTP/SOCKS5）
- 基于 IP、域名、用户认证的访问控制
- 结构化日志与可观测性（Prometheus metrics）
- 配置热加载（可选）
- 源码清晰，易于二次开发

### 1.3 非功能性目标
| 指标 | 目标 |
| :--- | :--- |
| 最大并发连接 | ≥ 10,000 |
| 平均附加延迟 | < 3ms (p99) |
| 内存占用（空闲） | < 30 MB |
| 内存占用（5k 连接） | < 400 MB |
| 吞吐量 | ≥ 1 Gbps |

---

## 2. 功能需求

| 模块 | 功能点 | 优先级 |
| :--- | :--- | :--- |
| **HTTP 代理** | 普通 HTTP 请求转发 | P0 |
| | HTTPS CONNECT 隧道（双向透传） | P0 |
| **SOCKS5 代理** | TCP CONNECT 命令 | P0 |
| | UDP ASSOCIATE（基础支持） | P1 |
| | 用户名/密码认证（RFC 1929） | P1 |
| **访问控制** | IP 黑白名单（CIDR 支持） | P0 |
| | 域名/URL 过滤（通配符，正则） | P1 |
| | 基于 HTTP 方法的过滤 | P2 |
| | 基础认证（Basic Auth） | P1 |
| **上游代理链** | 直接连接 | P0 |
| | 转发至上游 HTTP 代理 | P0 |
| | 转发至上游 SOCKS5 代理 | P0 |
| | 链式多跳代理 | P2 |
| **日志监控** | 访问日志（CLF / JSON） | P0 |
| | 错误日志（tracing） | P0 |
| | Prometheus metrics（连接数、流量、延迟） | P1 |
| **配置管理** | YAML 配置文件 | P0 |
| | 命令行参数覆盖 | P1 |
| | 热重载（SIGHUP 或 API） | P2 |
| **传输安全** | 支持 TLS 代理自身（HTTPS 代理） | P1 |
| | 上游 TLS 连接 | P1 |
| | DNS over HTTPS（防污染） | P2 |

---

## 3. 技术架构

### 3.1 整体架构图（无缓存）

```text
+-------------+      +------------------------------------------------+
| Client      |      |                 Peregrine                      |
| (Browser/   |----> |  +----------+   +-------------+   +----------+ |
| App)        |      |  |Listener  |-->|Protocol     |-->| Access   | |
+-------------+      |  |(Tokio)   |   |Detect&Parse |   | Control  | |
                     |  +----------+   +-------------+   +----------+ |
                     |        |                |               |      |
                     |        v                v               v      |
                     |  +----------+   +-----------------------------+ |
                     |  |  HTTP    |   |  SOCKS5                     | |
                     |  | Handler  |   |  Handler                    | |
                     |  +----------+   +-----------------------------+ |
                     |        |                |                       |
                     |        v                v                       |
                     |  +----------------------------------------+     |
                     |  |           Upstream Router              |     |
                     |  | (Direct / Parent Proxy / Chain)        |     |
                     |  +----------------------------------------+     |
                     |        |                                       |
                     |        v                                       |
                     |  +-------------+   +-------------+            |
                     |  | Response    |-->| Logging &   |            |
                     |  | Writer      |   | Metrics     |            |
                     |  +-------------+   +-------------+            |
                     +------------------------------------------------+
```

### 3.2 技术选型表

| 组件 | 技术选择 | 理由 |
| :--- | :--- | :--- |
| 异步运行时 | Tokio (full features) | 稳定、生态丰富、多线程工作窃取 |
| HTTP 协议 | Hyper 1.x | 高性能、安全、完整 HTTP/1.1 支持 |
| SOCKS5 实现 | 自研 (基于 tokio::net) | 轻量、可控、无外部依赖 |
| TLS | Rustls + webpki-roots | 纯 Rust，安全，免 OpenSSL |
| 配置解析 | Serde + serde_yaml | 强类型，易于校验 |
| 日志 | tracing + tracing-subscriber | 结构化、异步、可扩展 |
| 指标 | prometheus-client | 官方库，暴露 /metrics |
| 并发控制 | Arc + DashMap / tokio::sync | 高性能无锁数据结构 |
| 命令行解析 | clap | 功能丰富，文档自动生成 |

### 3.3 并发模型
- 每个客户端连接对应一个独立的 Tokio 任务（`tokio::spawn`）。
- 全局共享状态（ACL 规则、上游配置、认证用户表）使用 `Arc<dashmap::DashMap>` 或 `Arc<RwLock<T>>`。
- 限制最大并发连接：`tokio::sync::Semaphore` 初始化最大连接数。
- 每个连接内部使用 `tokio::io::copy_bidirectional` 进行双向数据复制（CONNECT 隧道或 SOCKS5 数据阶段）。

---

## 4. 模块详细设计

### 4.1 协议检测与分发模块

**职责**：接受新连接，读取首字节，识别协议类型（HTTP / SOCKS5 / 未知）。

**流程**：
1. `TcpListener::accept()` 获取 `TcpStream`。
2. 使用 `tokio::io::AsyncReadExt::peek` 读取前 2 字节而不消费。
3. 判断：
   - 第一个字节 == `0x05` → SOCKS5
   - 前 5 个字节匹配 `"GET /"`、`"POST "`、`"CONNEC"` 等 HTTP 方法前缀 → HTTP
   - 否则关闭连接（或记录错误）
4. 根据结果调用对应的 `handle_http(stream)` 或 `handle_socks5(stream)`。

**代码结构**：
```rust
pub async fn detect_and_dispatch(mut stream: TcpStream) {
    let mut buf = [0u8; 8];
    let _ = stream.peek(&mut buf).await?;
    match buf[0] {
        0x05 => handle_socks5(stream).await,
        _ if is_http_method_prefix(&buf) => handle_http(stream).await,
        _ => {
            tracing::warn!("unknown protocol, closing");
            let _ = stream.shutdown().await;
        }
    }
}
```

### 4.2 HTTP 代理模块

**支持特性**：
- 解析 HTTP 请求行和头部（使用 `hyper::Request`）。
- 对普通 HTTP 请求：构建新请求，转发到上游（直接或父代理），返回响应。
- 对 `CONNECT` 方法：建立到目标服务器的 TCP 连接，然后双向复制数据（隧道模式）。
- 支持 `Proxy-Authorization` 头部进行客户端认证。

**处理流程**：
1. 使用 `hyper::server::conn::http1::Builder::serve_connection` 与客户端交互。
2. 自定义 `Service` 实现 `call` 方法：
   - 检查访问控制（`AccessController::check(request)`）。
   - 若不是 CONNECT 方法：通过 `UpstreamRouter` 转发请求。
   - 若是 CONNECT 方法：调用 `handle_connect()` 建立隧道。
3. 响应返回前，记录访问日志和 metrics。

**关键代码片段**：
```rust
async fn handle_http(stream: TcpStream) {
    let service = ProxyService::new(acl, upstream_router);
    let conn = hyper::server::conn::http1::Builder::new()
        .serve_connection(stream, service);
    if let Err(e) = conn.await {
        tracing::error!("HTTP connection error: {}", e);
    }
}
```

### 4.3 SOCKS5 代理模块

**状态机实现** (RFC 1928, 1929)：

1. **协商阶段**：
   - 读取客户端支持的认证方法列表（`METHODS`）。
   - 选择 `0x00`（无认证）或 `0x02`（用户名/密码）。
   - 若选择 `0x02`，则进入认证子流程（读取用户名密码并验证）。
2. **请求阶段**：
   - 读取命令：`CONNECT` (`0x01`)，`BIND` (`0x02`)，`UDP ASSOCIATE` (`0x03`)。
   - 仅实现 `CONNECT` 和 `UDP ASSOCIATE`（BIND 可忽略或返回不支持）。
   - 解析目标地址类型（IPv4、域名、IPv6）。
3. **转发阶段**：
   - 对于 `CONNECT`：向上游或直接连接目标地址，然后 `tokio::io::copy_bidirectional`。
   - 对于 `UDP ASSOCIATE`：绑定 UDP socket 并回复客户端 UDP 中继地址，维护映射表（`DashMap<SocketAddr, UdpRelay>`）。

**认证实现**（可选）：
- 若配置了用户认证，在协商阶段回复 `0x02`，随后读取 `0x01` + 用户名长度 + 用户名 + 密码长度 + 密码。
- 验证通过后返回 `0x00`，否则 `0xff` 并关闭连接。

### 4.4 访问控制模块

**数据结构**：
```rust
pub enum AclAction {
    Allow,
    Deny,
    Authenticate,
}

pub struct AclRule {
    pub action: AclAction,
    pub src_ip: Option<Vec<IpNet>>,
    pub dst_domain: Option<Vec<GlobPattern>>,
    pub dst_port: Option<Range<u16>>,
    pub http_method: Option<HashSet<Method>>,
    pub auth_realm: Option<String>,
}
```

**检查流程**（按顺序匹配）：
1. 提取连接信息：源 IP、目标域名（从请求中解析）、目标端口、HTTP 方法（若是 HTTP）。
2. 遍历 ACL 规则，若所有条件匹配，则返回对应的动作。
3. 若为 `Allow`：继续。
4. 若为 `Deny`：返回 `403 Forbidden` 或 SOCKS5 拒绝错误。
5. 若为 `Authenticate`：发起 HTTP 401 挑战（仅 HTTP）或 SOCKS5 认证流程。

**性能优化**：使用 `RegexSet` 或 `Aho-Corasick` 进行多模式匹配；域名过滤支持通配符 (`*.example.com`)。

### 4.5 上游路由器（代理链）

**核心 Trait**：
```rust
#[async_trait]
pub trait UpstreamConnector: Send + Sync {
    async fn connect(
        &self,
        target: SocketAddrOrDomain,
        ctx: &RequestContext,
    ) -> Result<OwnedWriteHalf, ProxyError>;
    
    async fn send_http_request(
        &self,
        mut request: Request<Body>,
    ) -> Result<Response<Body>, ProxyError>;
}
```

**实现类型**：
- `DirectConnector`：直接使用 Tokio `TcpStream::connect`。
- `HttpProxyConnector`：通过 HTTP 父代理（支持 CONNECT 隧道或普通转发）。
- `Socks5ProxyConnector`：通过 SOCKS5 父代理。
- `ChainConnector`：组合多个连接器，依次连接。

**配置示例**：
```yaml
upstream:
  type: chain
  chain:
    - type: socks5
      addr: "192.168.1.100:1080"
      auth: { username: "user", password: "pass" }
    - type: direct
```

### 4.6 配置管理

**配置文件格式**（YAML）：
```yaml
server:
  listen:
    - addr: "0.0.0.0:8080"
      protocol: http
    - addr: "0.0.0.0:1080"
      protocol: socks5
  max_connections: 10000
  tcp_keepalive: 60

access_control:
  default_action: deny
  rules:
    - action: allow
      src_ip: ["192.168.0.0/16", "10.0.0.0/8"]
    - action: deny
      dst_domain: ["*.blocked.com", "porn.*"]
    - action: authenticate
      src_ip: ["0.0.0.0/0"]
      auth:
        type: basic
        users:
          - { username: "admin", password: "secret" }

upstream:
  type: http
  addr: "parent-proxy.example.com:3128"
  auth:
    username: "proxyuser"
    password: "proxypass"

logging:
  level: info
  access_log: "./access.log"
  format: json

metrics:
  enabled: true
  listen: "127.0.0.1:9090"
```

**加载与热重载**：
- 使用 `config` crate 或直接 `serde_yaml` + `std::fs`。
- 热重载方式：监听 `SIGHUP` 信号或暴露 `/reload` HTTP 端点（可选）。

### 4.7 日志与监控

- **访问日志**：使用 `tracing` 的 `info!` 宏，输出到文件（支持滚动）。
- **Metrics**：使用 `prometheus-client` 暴露：
  - `peregrine_connections_active` (Gauge)
  - `peregrine_requests_total` (Counter, with labels: protocol, action)
  - `peregrine_request_duration_seconds` (Histogram)
  - `peregrine_upstream_errors_total` (Counter)
- **健康检查**：可增加简单的 HTTP 端点 `/health` 返回 `200`。

---

## 5. 安全设计

| 威胁 | 防护措施 |
| :--- | :--- |
| 连接耗尽攻击 | 限制最大并发；每 IP 速率限制（令牌桶）；读/写超时（默认 30s） |
| 请求走私 | 使用 Hyper 严格解析，拒绝畸形 `Content-Length` / `Transfer-Encoding` |
| DNS 重绑定 | 对域名解析后检查 IP 是否属于内网（若禁止访问内网） |
| 中间人攻击（针对代理本身） | 支持 TLS 代理（HTTPS 代理）时，要求客户端信任自签名证书或配置有效证书链 |
| 日志注入 | 结构化日志，对用户输入转义 |
| 内存耗尽 | 限制单个请求体大小（默认 1MB），使用 `hyper::body::to_bytes` 限制 |

---

## 6. 错误处理与可靠性

- **连接层**：所有 `Result` 都应妥善处理，避免 panic。
- **超时控制**：使用 `tokio::time::timeout` 包裹连接和读取操作。
- **优雅关闭**：监听 `ctrl_c` 信号，停止接收新连接，等待已有连接完成（最多 30 秒）。
- **上游故障转移**（可选）：支持多个上游代理，自动选择可用节点。

---

## 7. 测试策略

| 测试类型 | 工具/方法 | 覆盖范围 |
| :--- | :--- | :--- |
| 单元测试 | `cargo test` | 协议解析、ACL 匹配、配置加载 |
| 集成测试 | `tokio::test` + 本地 mock 服务器 | HTTP 转发、CONNECT 隧道、SOCKS5 完整流程 |
| 性能测试 | `wrk`, `oha`, `hyperfine` | 吞吐量、延迟、最大连接数 |
| 模糊测试 | `cargo fuzz`（针对协议输入） | 协议解析器鲁棒性 |
| 内存泄漏 | `valgrind` / `heaptrack` | 长时间运行稳定性 |

---

## 8. 构建与部署

### 8.1 编译
```bash
cargo build --release
```

### 8.2 运行
```bash
./target/release/peregrine --config config.yaml
```

### 8.3 Docker 支持（可选）
```dockerfile
FROM rust:1.85 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/peregrine /usr/local/bin/
ENTRYPOINT ["peregrine"]
```

---

## 9. 项目结构

```
peregrine/
├── Cargo.toml
├── README.md
├── config.example.yaml
├── src/
│   ├── main.rs
│   ├── config.rs
│   ├── server.rs
│   ├── protocol/
│   │   ├── mod.rs
│   │   ├── http_handler.rs
│   │   └── socks5_handler.rs
│   ├── acl/
│   │   ├── mod.rs
│   │   └── rule.rs
│   ├── upstream/
│   │   ├── mod.rs
│   │   ├── direct.rs
│   │   ├── http_proxy.rs
│   │   └── socks5_proxy.rs
│   ├── metrics.rs
│   └── logging.rs
```

---

## 10. 开发计划（无缓存）

| 阶段 | 时间 | 交付内容 |
| :--- | :--- | :--- |
| 第1周 | 基础框架 | Tokio 监听 + 协议检测；HTTP 普通转发（无认证） |
| 第2周 | HTTP CONNECT + SOCKS5 | 支持隧道和 SOCKS5 TCP 连接 |
| 第3周 | 访问控制 | IP/域名过滤 + Basic 认证 |
| 第4周 | 上游代理 | 支持 HTTP 父代理和 SOCKS5 父代理 |
| 第5周 | 日志与监控 | tracing 集成，Prometheus 端点 |
| 第6周 | 集成测试与调优 | 压测、文档撰写、配置文件完善 |
| 第7周 | 预览版发布 | 二进制发布 + Docker 镜像 |

---

## 11. 附录：快速开始示例

**config.yaml**：
```yaml
server:
  listen:
    - addr: "0.0.0.0:8080"
      protocol: http

access_control:
  default_action: deny
  rules:
    - action: allow
      src_ip: ["192.168.1.0/24"]

upstream:
  type: direct

logging:
  level: debug
```

**启动**：
```bash
peregrine -c config.yaml
```

**客户端配置**：
- HTTP 代理地址：`192.168.1.100:8080`（运行 peregrine 的机器 IP）

---

> **总结**：游隼（Peregrine）是一款无缓存、专注转发效率的 Rust 代理软件。本文档提供了完整的架构设计、模块划分、安全考量和实现路径，可作为实际开发的蓝图。如需添加缓存功能，可参考上一份文档中的缓存引擎设计，但本项目初期不包含。
