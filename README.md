<h1 style="font-size: 48px;">别倒卖了😭</h1>

# NekoSportsWorldTool

![Release](https://img.shields.io/github/v/release/YanamiNeko/NekoSportsWorldTool)
![Build](https://github.com/YanamiNeko/NekoSportsWorldTool/actions/workflows/release.yml/badge.svg)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue)

运动世界校园版全协议自动化工具，纯 Rust 单文件，GUI / CLI 双模式。

> ⚠️ 仅供学习研究，请勿用于违反学校规定的场景，后果自负。

## 功能

- **跑步**：一键全链（策略 → 打卡点 → 轨迹 → 提交 → OBS → 验证），轨迹按打卡点自然生成，开始时间支持随机或指定到前 3 天
- **户外自由跑**：不拉取打卡点，轨迹绕定位锚点自然生成闭合环，其余策略/提交/OBS/验证流程一致（CLI `run-free` 或 GUI 切换「自由跑」）
- **AI 运动**：任务 / 自由练习双模式，批量补签（前 60 天 × 多项目）
- **数据**：学期完成度、违规自查、排行榜（个人/班级/院系 × 日/月、室内榜、历史榜）、个人主页
- **登录**：支持人机验证自动通过
- **自动更新**：桌面端检查/下载/自替换并重启（GitHub Release 直连，无镜像）；Android 下载 APK 调起系统安装器；启动检查模式可选（静默/询问/关闭）

## 界面

![GUI](docs/screenshot.png)

## 技术特点

- 完整实现 NetSecKit 信封加密与响应解密，每个响应强制 RSA 验签（fail-closed）
- 人机验证全程本地完成，不依赖任何外部服务
- 轨迹生成器：打卡点拟合环 + 自然速度曲线 + 哨兵点 / 断崖 / 点位吸附
- 设备身份持久化（DeviceId / 安装时间 / MAC），稳定不易触发风控
- GUI 与 CLI 共用同一套协议层，数据文件互通

## 使用

双击 exe 进入 GUI；CLI 用法：

```text
NekoSportsWorldTool login --user <手机号> --pass <密码> --remember
NekoSportsWorldTool run                                  # 一键跑步（--altitude 17.2 固定海拔，或 11.6-22.8 映射到区间）
NekoSportsWorldTool run-free                             # 户外自由跑（不拉取打卡点，绕锚点自由生成轨迹，参数同 run）
NekoSportsWorldTool ai --sport 2 --score 26000           # AI 运动
NekoSportsWorldTool rank main --type 1 --sort 1          # 排行榜
NekoSportsWorldTool update [--check]           # 自动更新（--check 仅检查不下载）
NekoSportsWorldTool template --file run.gpx   # 本地分析真实记录海拔（不会上传）
NekoSportsWorldTool help                                 # 全部命令
```

`template` 只读取用户手动选择的本地 GPX/JSON 文件，输出采样点、海拔范围、累计上升和累计下降，
不会登录、访问服务器或把模板记录接入跑步上传流程。

手动海拔支持三种状态：留空使用自动海拔；填写单个数字（例如 `17.2`）让所有轨迹点使用固定海拔；填写区间（例如 `11.6-22.8`）将生成的海拔曲线映射到该上下限内，同时保留轨迹的相对起伏。

青龙定时任务：首次手工 `login --remember` 一次，之后定时挂 `run` 即可。

## 常见问题

- **提示未登录 / 401**：先执行 `login --remember`（GUI 勾选记住密码等价），会话失效会自动重登
- **10121 设备风险**：设备身份是持久化的，删除 `identity.json` 等于换了新设备，不要频繁删
- **10603 点位限流**：点位接口 5 分钟限 3 次，程序内置 300s 缓存，正常使用不会触发
- **榜单为空**：当天还没有人产生有效里程，查询会自动回退最近 3 天
- **检查更新失败**：更新仅直连 GitHub，网络不通时到 [Release](https://github.com/YanamiNeko/NekoSportsWorldTool/releases) 手动下载
- **手动更新方式**：Windows 下载新的 `nekosportsworldtool.exe` 后退出软件，用新文件替换旧文件；请保留 `config.json`、`identity.json`、`points_cache.json` 等配置文件，不要删除或修改。
- **人机验证失败**：自动重试 3 轮，仍失败大概率是网络波动，稍后再试
- **杀软误报**：未签名编译产物可能被误报，自行判断后加白

## 构建

```bash
cargo build --release                        # GUI + CLI
cargo build --release --no-default-features  # 仅 CLI
```

推 tag 自动构建 Windows / macOS / Linux 四平台产物并发布 Release（见 `.github/workflows/release.yml`）。

## 更新日志

### v0.3.0 — 2026-10-20

**🎉 新增户外自由跑**

- 新增顶层 tab「自由跑」，与「跑步」并列。共享同一套参数控件，独立提交
- 自由跑不拉取校园打卡点，轨迹绕定位锚点自然生成闭合环
- 协议映射：`runMode=2`（策略）/ `sportType=4`（提交），官方 APP 正确显示「自由跑」独立标签，不重复计入校园跑学期成绩
- 校园跑保持 `runMode=1` / `sportType=1`（计分跑）不变

**🔧 RunMode / SportType 协议探测**

- GUI 新增「数据 → 运行模式探测」，一键枚举 runMode 0..=6 拉策略 + sportType 1..=6 提交探测
- 完整打印服务器解密响应，便于定位协议字段含义
- CLI 新增 `probe` 命令

**📱 Android APK 独立发布**

- 新增 `.github/workflows/apk.yml`，推 tag 自动构建 arm64-v8a APK（min API 26）

**📝 完整改动**

- `src/api/flow.rs`：RunMode 枚举 + policy_run_mode / submit_sport_type 分叉
- `src/api/submit.rs`：SubmitParams.sport_type 字段
- `src/api/policy.rs`：fetch_policy(client, run_mode) 参数化
- `src/ui/run.rs`：接受 `free: bool` 参数重构
- `src/cli/cmds.rs`：新增 `run-free` 和 `probe` 子命令

[完整 Release Notes →](RELEASE_NOTES.md)

## 许可证

[CC BY-NC 4.0](LICENSE) © YanamiNeko（禁止商用）
