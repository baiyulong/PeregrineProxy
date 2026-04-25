# GitHub Actions 快速参考

## 工作流概览

项目已配置自动化 CI/CD 流程，支持多平台构建。

## 🚀 快速开始

### 1. 触发自动构建

**选项 A：推送到分支**
```bash
git push origin feat/peregrine-proxy
```

**选项 B：创建发行版本标签**
```bash
git tag v1.0.0
git push origin v1.0.0
```

**选项 C：手动触发**
- 在 GitHub 仓库 → Actions 页面
- 选择 "Multi-Platform Build"
- 点击 "Run workflow"

### 2. 下载构建结果

**从 Actions 工件下载（临时，30天）**
- GitHub 仓库 → Actions → 最新工作流
- 点击 job（如 "Build ubuntu-latest - x86_64"）
- 下载 Artifacts

**从 Release 下载（永久）**
- 创建版本标签时自动上传
- GitHub 仓库 → Releases
- 下载对应平台的二进制文件

## 📋 工作流任务

| Job | 触发条件 | 功能 |
|-----|---------|------|
| **build** | 推送/PR | 5 个平台交叉编译 |
| **test** | 推送/PR | 单元测试 + 代码检查 |
| **docker-build** | 推送/标签 | Docker 镜像构建 |

## 🔧 支持的平台

```
✅ Linux x86_64      (native build)
✅ Linux aarch64     (cross-compile)
✅ macOS x86_64      (native build)
✅ macOS aarch64     (native build)
✅ Windows x86_64    (native build)
```

## 📦 生成的工件

### 构建工件命名
```
peregrine-<branch/tag>-<os>-<arch>.<ext>

示例：
- peregrine-v1.0.0-ubuntu-latest-x86_64.tar.gz
- peregrine-v1.0.0-macos-14-aarch64.tar.gz
- peregrine-v1.0.0-windows-latest-x86_64.zip
```

### 工件内容
```
peregrine-v1.0.0-ubuntu-latest-x86_64/
├── peregrine              # 可执行文件
├── config.example.yaml    # 配置示例
├── README.md              # 文档
├── LICENSE                # 许可证
└── Dockerfile             # Docker 配置
```

## 🐳 Docker 镜像

镜像自动推送到 GitHub Container Registry：

```bash
# 拉取最新镜像
docker pull ghcr.io/baiyulong/PeregrineProxy:latest

# 运行容器
docker run -d \
  -p 8080:8080 \
  -p 1080:1080 \
  -p 9090:9090 \
  -v /etc/peregrine:/etc/peregrine \
  ghcr.io/baiyulong/PeregrineProxy:v1.0.0
```

## 📊 监控构建状态

### Actions 页面
- GitHub 仓库 → Actions
- 查看实时构建进度
- 查看历史工作流运行

### 构建徽章
添加到 README：
```markdown
[![Multi-Platform Build](https://github.com/baiyulong/PeregrineProxy/actions/workflows/build.yml/badge.svg)](https://github.com/baiyulong/PeregrineProxy/actions/workflows/build.yml)
```

## ⚙️ 自定义配置

### 修改支持的平台

编辑 `.github/workflows/build.yml`，在 `matrix.include` 中添加/移除平台：

```yaml
matrix:
  include:
    - os: ubuntu-latest
      arch: x86_64
      target: x86_64-unknown-linux-gnu
```

### 添加构建变量

```yaml
env:
  CARGO_TERM_COLOR: always
  MY_VAR: "value"
```

### 修改 Docker 推送目标

```yaml
images: |
  ghcr.io/${{ github.repository }}
  docker.io/username/peregrine
```

## 🔐 密钥配置（可选）

### Docker Hub 推送

1. 在 GitHub 仓库 → Settings → Secrets and variables → Actions
2. 添加密钥：
   - `DOCKER_USERNAME` - Docker Hub 用户名
   - `DOCKER_PASSWORD` - Docker Hub Access Token

获取 Access Token：
- https://hub.docker.com → Account Settings → Security
- 创建 Personal Access Token

## 📈 构建性能

### 缓存效果

首次构建：
- Linux: ~5-10 分钟
- macOS: ~10-15 分钟  
- Windows: ~10-15 分钟

后续构建（缓存命中）：
- Linux: ~2-3 分钟
- macOS: ~3-5 分钟
- Windows: ~3-5 分钟

### 优化建议

1. 提交前本地测试
2. 合并小型提交避免重复构建
3. 使用 Release 标签进行正式发布

## 🚨 故障排查

### 构建失败

1. 查看 Actions 页面详细日志
2. 常见原因：
   - 编译错误 → 修复代码
   - 测试失败 → 检查单元测试
   - 依赖问题 → 检查 Cargo.toml

### Docker 构建失败

- 检查 Dockerfile 语法
- 确保 Docker Hub 凭证正确
- 查看 Docker 构建日志

### 工件下载失败

- 确认工作流已完成
- 检查工件保留期（30天）
- 使用 Release 工件做备份

## 📚 相关资源

- [build.yml 详细说明](.github/workflows/README.md)
- [GitHub Actions 官方文档](https://docs.github.com/en/actions)
- [Rust 交叉编译指南](https://rust-lang.github.io/rustup/cross-compilation.html)
- [Docker Buildx 文档](https://github.com/docker/buildx)

## 💡 最佳实践

1. **版本管理**
   - 使用语义化版本：v1.0.0, v1.0.1, v1.1.0
   - 每个标签生成一个 Release

2. **发布流程**
   ```bash
   # 更新版本
   # 提交变更
   git commit -m "bump version to v1.0.0"
   
   # 创建标签
   git tag v1.0.0
   
   # 推送
   git push origin v1.0.0
   
   # 等待 GitHub Actions 完成
   # 在 Releases 页面检查工件
   ```

3. **提交规范**
   - feat: 新功能
   - fix: 修复
   - ci: CI/CD 变更
   - docs: 文档

---

**上次更新**：2026-04-25
**工作流文件**：`.github/workflows/build.yml`
