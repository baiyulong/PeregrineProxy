# GitHub Actions 自动构建指南

## 概述

`.github/workflows/build.yml` 配置了一个完整的多平台自动构建流程，支持以下平台和架构：

### 支持的平台

| 平台 | 架构 | 说明 |
|------|------|------|
| Linux | x86_64 | Intel/AMD 64位 Linux |
| Linux | aarch64 | ARM 64位 Linux（使用 cross 工具链）|
| macOS | x86_64 | Intel 64位 macOS |
| macOS | aarch64 | Apple Silicon (M1/M2/M3+) |
| Windows | x86_64 | Intel/AMD 64位 Windows |

## 工作流功能

### 1. 自动构建 (build job)

- 在以下情况触发：
  - Push 到 `main` 或 `feat/**` 分支
  - 创建版本标签（`v*`）
  - Pull Request 到 `main` 分支
  - 手动触发 (`workflow_dispatch`)

- 为每个平台构建 Release 版本
- 使用 Cargo 缓存加速构建
- 对 Linux/macOS 二进制文件进行 Strip 以减小体积
- 创建包含以下内容的发行版压缩包：
  - `peregrine` 可执行文件
  - `config.example.yaml` 配置示例
  - `README.md` 使用文档
  - `LICENSE` 许可证
  - `Dockerfile` Docker 配置

### 2. 测试和检查 (test job)

- 运行所有单元测试：`cargo test --verbose --all`
- 代码检查：`cargo clippy --all --all-targets`
- 格式检查：`cargo fmt --all -- --check`

### 3. Docker 镜像构建 (docker-build job)

- 构建多架构 Docker 镜像
- 推送到 GitHub Container Registry (GHCR)
- 可选：推送到 Docker Hub（需配置 `DOCKER_USERNAME` 和 `DOCKER_PASSWORD` 密钥）

## 自动生成的工件

### GitHub Actions 工件

每个构建完成后，工件保留 30 天，可在 Actions 页面下载：

```
peregrine-<branch>-<os>-<arch>.tar.gz    (Linux/macOS)
peregrine-<branch>-<os>-<arch>.zip      (Windows)
```

### Release 工件

创建版本标签时（例如 `v1.0.0`），自动上传至 GitHub Release：

```
peregrine-v1.0.0-ubuntu-latest-x86_64.tar.gz
peregrine-v1.0.0-ubuntu-latest-aarch64.tar.gz
peregrine-v1.0.0-macos-12-x86_64.tar.gz
peregrine-v1.0.0-macos-14-aarch64.tar.gz
peregrine-v1.0.0-windows-latest-x86_64.zip
```

## 必需的仓库密钥 (Secrets)

### 可选 - Docker Hub 推送

在 GitHub 仓库设置中添加：

1. `DOCKER_USERNAME` - Docker Hub 用户名
2. `DOCKER_PASSWORD` - Docker Hub 访问令牌（不是密码）

获取访问令牌：
- 登录 https://hub.docker.com
- 进入 Account Settings → Security
- 创建新的 Personal Access Token

### 必需 - GitHub Token

默认由 GitHub 提供的 `GITHUB_TOKEN` 自动可用，无需手动配置。

## 使用示例

### 1. 推送代码自动构建

```bash
git push origin feat/peregrine-proxy
```

在 GitHub Actions 页面查看构建进度，完成后下载工件。

### 2. 创建发行版本

```bash
git tag v1.0.0
git push origin v1.0.0
```

- 工作流自动构建所有平台
- 生成的二进制文件自动上传到 Release
- Docker 镜像自动推送到仓库

### 3. 手动触发构建

在 GitHub 仓库 Actions 页面，选择 "Multi-Platform Build"，点击 "Run workflow"。

## 工作流配置详解

### 构建缓存

使用 GitHub 缓存加速后续构建：
- Cargo registry 缓存（依赖）
- Cargo 构建缓存（编译产物）

### 跨平台工具

**Linux ARM64 构建**

使用 `cross` 工具链进行交叉编译：

```bash
cargo install cross
cross build --release --target aarch64-unknown-linux-gnu
```

### 二进制优化

- **Strip**: Linux/macOS 使用 `strip` 移除符号表，减小 50-70%
- **分发包**: 包含必要的配置文件和文档

### Docker 镜像

自动生成标签：
- `main` 分支 → `latest`
- 版本标签 `v1.0.0` → `1.0.0`, `1.0`, `latest`
- 分支名称 → 分支标签
- 提交 SHA → `sha-<commit>`

## 常见问题

### Q: ARM64 构建为什么较慢？

A: Linux ARM64 使用 `cross` 工具链进行交叉编译，较为耗时。可通过 `matrix.max-parallel` 限制并行数量。

### Q: 如何只构建特定平台？

A: 修改 `build.yml` 的 `matrix.include` 部分，注释掉不需要的平台。

### Q: 能否自定义构建参数？

A: 可以。修改 `cargo build` 命令行，例如：
```yaml
- name: Custom build
  run: cargo build --release --target ${{ matrix.target }} --features custom-feature
```

### Q: Docker 镜像多久更新一次？

A: 每次 Push 到 `main` 分支或创建标签时自动构建。

## 监控和调试

### 查看构建日志

1. 进入 GitHub 仓库 → Actions
2. 选择最近的工作流运行
3. 点击具体的 job 查看详细日志

### 调试失败的构建

常见原因：
- 缺少依赖
- 编译错误
- 测试失败
- 权限问题

查看完整日志获取具体错误信息。

## 后续优化建议

1. **构建时间优化**
   - 启用增量编译
   - 并行化测试

2. **存储优化**
   - 压缩二进制文件
   - 使用 UPX (Ultimate Packer for eXecutables)

3. **发布自动化**
   - 自动生成 CHANGELOG
   - 自动创建 Release Notes

4. **版本管理**
   - 自动版本号管理
   - Git 标签自动化

## 相关文档

- [GitHub Actions 文档](https://docs.github.com/en/actions)
- [Rust cross 工具链](https://github.com/cross-rs/cross)
- [Docker GitHub Actions](https://github.com/docker/build-push-action)
