# 上游 API Key / 模型验货工具

这是一个本地验货工具，用于检测上游供应商提供的 API Key 和模型是否符合接入要求。项目同时提供桌面端和 CLI 端，两者共用同一套 Rust 后端核心逻辑，避免判断标准在不同 UI 中分叉。

## 功能概况

当前工具主要检测：

- 基础聊天能力
- Tools / Function Calling 能力
- Stream 流式响应能力
- JSON Mode / 结构化输出能力
- 错误格式是否接近官方协议
- 疑似逆向、中转、裁剪、不稳定供货风险
- PASS / WARN / FAIL 结论
- JSON / Markdown / 摘要报告输出

已支持的协议类型：

- `openai-compatible`：OpenAI-compatible Chat Completions
- `openai-responses`：OpenAI Responses API
- `anthropic-messages`：Anthropic Messages API
- `google-gemini`：Google Gemini API

协议类型可根据模型名自动推导。比如：

- `gpt-5.5` 推导为 `openai-responses`
- `gpt-4o` 推导为 `openai-compatible`
- `claude-*` 推导为 `anthropic-messages`
- `gemini-*` 推导为 `google-gemini`

### 检测性能：单协议内部并行

单个协议的检测流程为：先跑「基础聊天」，通过后再并行执行「Tools / Stream / JSON Mode」三项，最后跑「错误格式」并汇总风险评分。

- 基础聊天通过后三项并行，明显缩短单次检测耗时。
- 基础聊天失败时，直接跳过后续三项（标记为「已跳过」），避免对疑似不可用的上游重复请求。
- 该优化在 `probe-core` 中实现，桌面端和 CLI 无需改动即可受益。

### 多协议同时验货

一个模型可以一次性对多个协议验货，尤其适合 OpenAI 系模型可能同时支持 `openai-compatible` 与 `openai-responses` 的场景。

- 协议之间串行执行（避免瞬时对同一上游并发过多请求），每个协议内部仍然并行。
- 综合结论取各协议中的最优结果（任一协议 PASS 即综合 PASS），并标注「表现最佳协议」。
- 桌面端协议选择改为多选复选框；CLI 通过重复 `--protocol` 或逗号分隔传入多个协议，交互模式下用空格多选。
- 多协议报告支持导出 JSON / Markdown，包含各协议结论概览表与每个协议的完整明细。

## 桌面端截图

![桌面端检测页](docs/screenshots/desktop-main.svg)

截图展示的是桌面端主检测页：左侧填写验货参数，右侧展示检测进度和报告摘要。完整报告会进入二级详情页查看。

## 技术栈

- 桌面端：Tauri 2 + React + TypeScript + Vite
- 后端核心：Rust
- HTTP 请求：`reqwest`
- CLI：Rust binary
- 打包：Tauri Bundler + GitHub Actions

## 项目架构

项目是一个 Cargo workspace：

```text
.
├── Cargo.toml
├── package.json
├── src/                         # React 桌面端 UI
├── src-tauri/                   # Tauri 桌面壳
├── crates/
│   ├── probe-core/              # 共用后端核心逻辑
│   └── probe-cli/               # CLI 交互层
└── .github/workflows/           # GitHub Actions 打包配置
```

### `crates/probe-core`

核心库，桌面端和 CLI 都依赖它。

负责：

- 协议推导：`infer_protocol_type`
- 探针执行：`run_probe`
- JSON 报告：`to_json`
- Markdown 报告：`to_markdown`
- 摘要报告：`to_summary`
- 风险评分
- 各协议请求和响应解析

原则：所有业务判断、协议适配、风险评分、报告格式化都尽量放在 `probe-core`。

### `src-tauri`

桌面端 Tauri 壳。

负责：

- 注册 Tauri command
- 转发 React invoke 到 `probe-core`
- 推送检测进度事件
- 调用系统文件保存能力

不负责业务判断。

### `src`

React UI。

负责：

- 表单输入（协议类型为多选复选框，可一次选多个协议）
- 检测进度展示（多协议时按协议分组显示各自进度）
- 报告摘要展示（多协议时展示综合结论与各协议结论）
- 报告详情页（单协议或多协议明细）
- 导出按钮和桌面文件夹选择

协议推导通过 Tauri command 调用 `probe-core`，不在前端重复维护规则。

### `crates/probe-cli`

命令行 UI。

负责：

- 终端交互输入
- 空格多选协议类型（可多选），上下键选择输出格式、退出码阈值
- 参数解析（`--protocol` 可重复或逗号分隔以测试多个协议）
- 调用 `probe-core`（单协议走 `run_probe`，多协议走 `run_multi_protocol_probe`）
- 输出到终端或文件

不负责探针业务判断。

## 本地环境准备

需要安装：

- Node.js 22 或更新版本
- npm
- Rust stable
- macOS 本地桌面打包需要 Xcode Command Line Tools
- Windows 桌面打包建议在 Windows 或 GitHub Actions `windows-latest` 上执行

安装前端依赖：

```bash
npm install
```

检查 Rust workspace：

```bash
cargo check --workspace
```

## 桌面端本地启动

开发模式：

```bash
npm run tauri dev
```

如果需要走本地代理，可以在启动前设置：

```bash
export https_proxy=http://127.0.0.1:7897
export http_proxy=http://127.0.0.1:7897
npm run tauri dev
```

前端单独构建：

```bash
npm run build
```

Tauri 不打包构建检查：

```bash
npx tauri build --no-bundle
```

## 桌面端本地打包

macOS DMG：

```bash
npx tauri build --bundles dmg
```

产物位置：

```text
target/release/bundle/dmg/*.dmg
```

Windows EXE 安装包需要在 Windows 环境执行：

```bash
npx tauri build --bundles nsis
```

产物位置：

```text
target/release/bundle/nsis/*.exe
```

注意：本地未签名包在 macOS 或 Windows 上可能出现安全提示。测试时 macOS 可右键应用选择“打开”；正式分发需要考虑签名和公证。

## CLI 本地使用

构建 CLI：

```bash
cargo build -p apikey-probe-cli --release
```

可执行文件：

```bash
./target/release/apikey-probe
```

直接运行会进入交互式向导：

```bash
./target/release/apikey-probe
```

交互中会依次输入或选择：

- Base URL
- API Key，隐藏输入，不回显
- 模型名
- 协议类型，上下键选择，默认根据模型名推导
- 供应商名称
- 代理地址
- 备注
- 输出格式，上下键选择
- 输出文件路径，仅 JSON / Markdown 时出现
- 退出码失败阈值，上下键选择

输出格式：

- 摘要：直接输出到终端
- JSON 报告：默认输出 `report.json`
- Markdown 报告：默认输出 `report.md`

脚本化调用也支持，参数直接传给根命令，不需要子命令：

```bash
./target/release/apikey-probe \
  --base-url https://api.example.com/v1 \
  --api-key-env UPSTREAM_API_KEY \
  --model gpt-4o \
  --protocol auto \
  --format markdown \
  --out report.md
```

也可以从 stdin 传 API Key：

```bash
printf "%s" "$UPSTREAM_API_KEY" | ./target/release/apikey-probe \
  --base-url https://api.example.com/v1 \
  --api-key-stdin \
  --model claude-3-5-sonnet-latest
```

查看帮助：

```bash
./target/release/apikey-probe --help
```

### CLI 使用示例

#### 交互式检测

最适合人工本地验货：

```bash
./target/release/apikey-probe
```

示例交互：

```text
上游 API Key / 模型验货 CLI
按回车可使用括号内默认值，API Key 输入不会回显。

Base URL: https://api.example.com/v1
API Key:
模型名: gpt-5.5
已根据模型名推测协议类型：openai-responses
协议类型: OpenAI Responses API
供应商名称: 示例供应商
代理地址 (例如 http://127.0.0.1:7890):
备注: 第一轮验货
输出格式: Markdown 报告
输出文件路径 [report.md]:
退出码失败阈值: 仅 FAIL 时返回失败退出码
```

#### 生成 Markdown 报告

```bash
./target/release/apikey-probe \
  --base-url https://api.example.com/v1 \
  --api-key-env UPSTREAM_API_KEY \
  --model gpt-5.5 \
  --protocol auto \
  --format markdown \
  --out report.md
```

#### 生成 JSON 报告

```bash
./target/release/apikey-probe \
  --base-url https://api.example.com/v1 \
  --api-key-env UPSTREAM_API_KEY \
  --model claude-3-5-sonnet-latest \
  --protocol auto \
  --format json \
  --out report.json
```

#### 一次对多个协议验货

对同一模型同时测试 `openai-compatible` 与 `openai-responses`，重复 `--protocol` 或逗号分隔均可：

```bash
./target/release/apikey-probe \
  --base-url https://api.example.com/v1 \
  --api-key-env UPSTREAM_API_KEY \
  --model gpt-4o \
  --protocol openai-compatible \
  --protocol openai-responses \
  --format markdown \
  --out report.md

# 等价写法
./target/release/apikey-probe \
  --base-url https://api.example.com/v1 \
  --api-key-env UPSTREAM_API_KEY \
  --model gpt-4o \
  --protocol openai-compatible,openai-responses \
  --format markdown \
  --out report.md
```

多协议摘要输出示例：

```text
综合结论：PASS
模型：gpt-4o
表现最佳协议：openai-compatible
结论说明：建议接入：至少一种协议下基础聊天、tools、stream、JSON mode 等核心能力通过。

各协议结论：
- [PASS] openai-compatible（风险 0，LOW）
- [WARN] openai-responses（风险 15，LOW）
```

#### 只在终端输出摘要

```bash
./target/release/apikey-probe \
  --base-url https://api.example.com/v1 \
  --api-key-env UPSTREAM_API_KEY \
  --model qwen-plus \
  --format summary
```

摘要输出示例：

```text
结论：WARN
模型：qwen-plus
协议：openai-compatible
风险评分：35（MEDIUM）
结论说明：基础可用，但能力不完整或存在风险信号，建议人工复核后再接入。

检测项：
- [PASS] 基础聊天: 基础响应正常
- [WARN] Tools / Function Calling: 工具调用能力不完整
```

#### 从 stdin 传入 API Key

适合不想把 API Key 放进命令行参数或环境变量的场景：

```bash
printf "%s" "$UPSTREAM_API_KEY" | ./target/release/apikey-probe \
  --base-url https://api.example.com/v1 \
  --api-key-stdin \
  --model gemini-1.5-pro \
  --format markdown \
  --out report.md
```

#### CI 中失败阈值更严格

如果 WARN 也要让 CI 失败：

```bash
./target/release/apikey-probe \
  --base-url https://api.example.com/v1 \
  --api-key-env UPSTREAM_API_KEY \
  --model deepseek-chat \
  --format json \
  --out report.json \
  --fail-on warn
```

### CLI 参数说明

常用参数：

- `--base-url <url>`：上游 Base URL
- `--model <name>`：模型名
- `--protocol <value>`：协议类型，默认 `auto`（按模型名推导）；可重复传入或逗号分隔以同时测试多个协议，例如 `--protocol openai-compatible --protocol openai-responses`
- `--api-key-env <name>`：从环境变量读取 API Key，推荐脚本使用
- `--api-key-stdin`：从 stdin 读取 API Key
- `--api-key <key>`：直接传 API Key，不推荐，会进入 shell history
- `--provider-name <name>`：供应商名称
- `--proxy-url <url>`：代理地址
- `--note <text>`：备注
- `--format <summary|json|markdown>`：输出格式
- `--out <path>`：输出文件路径
- `--fail-on <fail|warn|never>`：控制退出码失败阈值

### CLI 退出码

- `0`：检测通过，或未达到失败阈值
- `1`：结论为 WARN 且 `--fail-on warn`
- `2`：结论为 FAIL
- `64`：参数错误
- `70`：运行时错误

## GitHub Actions 打包

仓库提供两条打包 workflow。

### 桌面客户端

Workflow：`Desktop Client Build`

触发方式：

- GitHub 页面手动 `Run workflow`
- 推送 `v*` tag

产物：

- `desktop-windows-exe`：Windows `.exe` 安装包
- `desktop-macos-dmg`：macOS `.dmg` 安装包

### CLI

Workflow：`CLI Build`

触发方式：

- GitHub 页面手动 `Run workflow`
- 推送 `v*` tag

产物：

- `apikey-probe-windows`：Windows CLI，可执行文件 `apikey-probe.exe`
- `apikey-probe-macos`：macOS CLI，可执行文件 `apikey-probe`
- `apikey-probe-linux`：Linux CLI，可执行文件 `apikey-probe`

## 验证命令

修改 core 或 CLI 后建议运行：

```bash
cargo test -p apikey-probe-core infer
cargo check --workspace
```

修改桌面 UI 后建议运行：

```bash
npm run build
```

修改 Tauri 相关逻辑后建议运行：

```bash
npx tauri build --no-bundle
```

## 注意事项

- 不要提交真实 API Key。
- `upstream-model-*` 已在 `.gitignore` 中忽略，用于避免误提交上游数据导出。
- 默认 CLI 输出文件 `report.md`、`report.json` 已忽略。
- `node_modules/`、`dist/`、`target/`、本地 Cargo cache 都不应提交。
- 桌面端和 CLI 的业务判断必须优先放到 `crates/probe-core`，不要在 UI 层重复实现。
- 协议自动推导规则只维护在 `probe-core::infer_protocol_type()`。
- 如果新增协议，需要同步更新 core 探针、前端协议选项、CLI 交互选项和报告说明。
- 正式对外分发前需要考虑 macOS 签名/公证、Windows 代码签名和版本号管理。
