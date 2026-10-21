# NekoSportsWorldTool v0.3.0 Release Notes

> 发布日期：2026-10-20
> 提交：`3e6b4aa`

## 🎉 亮点

### 1. 全新「户外自由跑」模式

之前工具只支持打卡点校园跑（必须经过指定的校园点位，绕圈闭合）。很多场景下想随便找条路/公园/操场跑一段，又不想被打卡点约束——现在可以了。

**参数一致，流程独立**：和校园跑共用同一套距离/配速/海拔/开始时间控制，区别只在于：
- 不拉取打卡点区域（`runAreaModels = null`）
- 轨迹绕定位锚点自然生成闭合环，不吸附任何校园点位
- 提交走独立的 `sportType=4`，服务器端不重复计入校园跑学期成绩

**实测验证**：官方 APP 记录列表正确显示「自由跑 达标」标签，与原有「计分跑」分类独立。

### 2. RunMode / SportType 协议探测（GUI + CLI）

第一次上手或协议字段有变化时，直接跑探测：

```bash
# GUI：数据 → 运行模式探测
# CLI
NekoSportsWorldTool probe
```

一键枚举 runMode 0..=6 和 sportType 1..=6，完整打印服务器解密后的策略响应，帮助定位参数含义。本次自由跑的 `sportType=4` 就是通过探测锁定的。

### 3. Android APK 独立发布

新增 `.github/workflows/apk.yml`，推 tag 自动构建 arm64-v8a APK（min API 26 / Android 8+），debug 签名，可直接下载到手机安装。

## 🔧 技术实现

### 协议层

| 模式 | runMode（策略） | sportType（提交） | 服务器端标签 |
|------|----------------|-------------------|--------------|
| 校园打卡点跑 | 1 | 1 | 计分跑（计入学期成绩） |
| 户外自由跑 | 2 | 4 | 自由跑（独立分类） |

RunMode 枚举在 `src/api/flow.rs`：

```rust
pub enum RunMode { Checkpoint, Free }

impl RunMode {
    fn policy_run_mode(self) -> i64 {
        match self { RunMode::Checkpoint => 1, RunMode::Free => 2 }
    }
    fn submit_sport_type(self) -> i64 {
        match self { RunMode::Checkpoint => 1, RunMode::Free => 4 }
    }
}
```

### UI 层

`src/ui/run.rs` 重构为接受 `free: bool` 参数，两个顶层 tab（「跑步」和「自由跑」）共享同一组参数控件，仅在 header 显示当前模式。

## 📦 包含产物

| 平台 | 文件 | 说明 |
|------|------|------|
| Windows (x64) | `nekosportsworldtool.exe` | GUI + CLI 一体 |
| macOS (arm64) | `nekosportsworldtool` | Apple Silicon |
| macOS (x64) | `nekosportsworldtool` | Intel |
| Linux (x64) | `nekosportsworldtool` | glibc 2.34+ |
| Android (arm64) | `NekoSportsWorldTool-android-arm64.apk` | min API 26 |

## 🐛 修复与改进

- 修复 envelope 响应解密在部分服务器配置下的边界 case
- 轨迹生成器增加自然速度曲线权重，避免配速异常
- session 失效时自动重登，无需手动重新登录

## 📚 构建

```bash
cargo build --release                        # GUI + CLI
cargo build --release --no-default-features  # 仅 CLI

# Android
export NDK_BIN=/opt/android-sdk/ndk/29.0.14206865/toolchains/llvm/prebuilt/linux-x86_64/bin
./android/release.ps1
```

## 📄 许可证

[CC BY-NC 4.0](LICENSE) — 非商用、保留署名、禁止二次闭源分发。

## 🙏 致谢

原作者 YanamiNeko 提供的完整协议层实现（NetSecKit 信封加密 + 响应 RSA 验签 + 轨迹生成器 + GUI），本次只是在正确的位置新增了模式分支。自由跑的 sportType=4 通过与官方 APP 实测对比锁定。
