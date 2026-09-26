# cargo-eta

> 一个 Cargo 子命令：在构建过程中**实时显示进度**，并给出**预计剩余时间（ETA）**——而且越用越准。

Cargo 只告诉你*正在*编译哪个 crate，却不告诉你整体进行到哪一步了，也不告诉你还要等多久。`cargo-eta` 补上这块：它包装 `cargo build` / `check` / `test` / `clippy`，消费 Cargo 的机器可读 JSON 消息流，渲染一个带 ETA 的进度条。

真正有意思的部分是 ETA。不同 crate 的编译耗时天差地别，所以「已完成数 ÷ 总数 × 已用时间」这种朴素外推会严重失真。`cargo-eta` 维护一份**持久化的 per-crate 耗时模型**，用它来预估尚未完成的编译单元。

## ETA 是怎么算的

任意时刻，每个计划中的编译单元处于四种状态之一，总预估耗时是它们各自的贡献之和：

| 状态     | 对预估的贡献                                          |
|----------|-------------------------------------------------------|
| 已完成   | 它的*真实*墙钟耗时                                     |
| 进行中   | `预期耗时 − 已耗时`（若已超时，则按预期的一半兜底）    |
| 未开始   | 该 crate 的历史 `预期耗时`                             |
| 缓存命中 | 零（Cargo 复用了已有的编译产物）                       |

```
总预估耗时 = 已完成真实耗时
           + Σ 进行中单元的剩余时间
           + Σ 未开始单元的预期耗时
ETA        = 总预估耗时 − 已用时间
```

`预期耗时` 来自持久化模型。遇到从没见过的 crate，用一个保守的默认值，这样首次构建的 ETA 会偏「太久」而不是偏「太快」。

## 耗时模型

模型存放在 `~/.cargo/eta-cache.json`（可用 `CARGO_HOME` 覆盖）。每个编译单元由以下维度共同标识：

- crate 名称与版本
- 构建 profile（`dev` / `release` / …）
- 目标平台（target triple）
- 是否为 proc-macro

一次**成功**的构建结束后，每个单元的真实耗时用指数加权移动平均（α = 0.3）折回模型，所以单次异常慢的构建——机器卡顿、杀毒扫描——只会轻微影响估计，而不会污染它。

**失败**的构建完全不更新模型：失败构建的耗时没有代表性。

## 安装

```sh
cargo install --path .
```

这会生成一个 `cargo-eta` 可执行文件。由于 Cargo 会在 `PATH` 上寻找 `cargo-<name>` 形式的可执行文件，因此可以用两种方式调用：

```sh
cargo eta build        # 作为 cargo 子命令
cargo-eta build        # 直接调用
```

## 用法

```
cargo eta <SUBCOMMAND> [CARGO FLAGS...]
cargo-eta <SUBCOMMAND> [CARGO FLAGS...]

子命令：
    build, check, test, clippy

选项：
    -q, --quiet                只输出最终总结
        --json                 输出机器可读的进度事件流
        --eta-update-every <N> 每 N 个编译单元刷新一次进度（默认 1）
    -h, --help                 显示帮助
    -V, --version              显示版本
```

其余任何参数都会原样透传给底层的 cargo 调用，例如：

```sh
cargo eta build --release
cargo eta check --all-targets
cargo eta test --no-run
```

### 输出模式

- **交互式终端** —— 在 stderr 上渲染一个实时进度条，显示百分比、已完成/总单元数、ETA、已用时间，以及当前正在编译的 crate。
- **非 TTY（CI 日志）** —— 进度条替换为节流的普通行输出（大约每 5 秒一行），保证日志可读：
  ```
  [cargo-eta]  42% (37/88) elapsed 12s eta 18s | syn
  ```
- **`--quiet`** —— 不输出逐单元进度，只给最终总结。
- **`--json`** —— 在 stdout 上输出 JSON 进度事件流，供 CI 或其它工具集成：
  ```json
  {"type":"progress","percent":42.05,"done":37,"total":88,"running":2,"fresh":0,"elapsed":12.004,"eta":18.21,"current":"syn"}
  {"type":"finished","success":true,"elapsed":30.77,"total":88,"done":88}
  ```

## 从 0% 起就准确的百分比

要算出百分比，工具需要一开始就知道总编译单元数。它会尝试 `cargo <subcommand> --unit-graph -Z unstable-options`，而这是**仅 nightly 可用**的。当该方式不可用时（stable 工具链），它会回退到「按事件出现的顺序动态发现编译单元」——构建照常可用，但早期的百分比是近似的。回退行为会在 stderr 上提示。

## 为什么用 JSON 事件而不是解析文本

`cargo-eta` 从不解析 Cargo 的人类可读输出，那太脆弱了。它始终以 `--message-format=json` 运行 Cargo，并响应 `compiler-artifact`、`build-script-executed`、`build-finished` 这几类事件。

## 状态

早期原型。build/check/test/clippy 路径、持久化模型，以及三种输出模式均已实现；模型的预测精度尚未在大型 workspace 上做过基准测试。

## 许可证

MIT OR Apache-2.0。