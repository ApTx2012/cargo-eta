# cargo-eta

> 一个 Cargo 子命令：在构建过程中**实时显示进度**，并给出**预计剩余时间（ETA）**——而且越用越准。

Cargo 只告诉你*正在*编译哪个 crate，却不告诉你整体进行到哪一步了，也不告诉你还要等多久。`cargo-eta` 补上这块：它包装 `cargo build` / `check` / `test` / `clippy`，消费 Cargo 的机器可读 JSON 消息流，渲染一个带 ETA 的进度条。

真正有意思的部分是 ETA。不同 crate 的编译耗时天差地别，所以「已完成数 ÷ 总数 × 已用时间」这种朴素外推会严重失真。`cargo-eta` 维护一份**持久化的 per-crate 耗时模型**，用它来预估尚未完成的编译单元。

## 快速上手（小白向）

### 这是干什么的？

你敲 `cargo build` 编译一个 Rust 项目时，Cargo 只会一行行地报「正在编译 xxx」，但你**看不到总进度**，也**不知道还要等多久**。依赖一多，就只能干瞪着屏幕猜。

`cargo-eta` 就是来解决这个的。装上它之后，你会看到一个**实时的进度条**，像这样：

```
⠹ [=========>--------------] 37/88 (42%) | ETA 18s | 12s | syn
```

左边是进度百分比，中间是「还要等 18 秒」，右边是「已经花了 12 秒」，最后是「正在编译 syn 这个库」。

而且——**它会记住每个库上次编译花了多久**。所以你第二次、第三次构建同一个项目时，那个「还要等多久」会越来越准。

### 三步用起来

**前提**：你得先装了 Rust 工具链（也就是 `cargo` 命令能用）。没装的话去 <https://rustup.rs> 按提示装。

**第一步：安装 cargo-eta**

```sh
cargo install --git https://github.com/ApTx2012/cargo-eta
```

> 如果这条命令报错说找不到 git 或认证失败，也可以先把仓库克隆到本地，进到目录里执行 `cargo install --path .`。

**第二步：进入你的 Rust 项目目录**

```sh
cd 你的项目
```

**第三步：把平时的 `cargo build` 换成 `cargo eta build`**

```sh
cargo eta build
```

就这样。之前 `cargo build` 后面能跟的参数，现在照抄在后面就行：

```sh
cargo eta build --release        # 跟 cargo build --release 一样
cargo eta check                  # 只做检查，不生成可执行文件
cargo eta test                   # 编译并跑测试
cargo eta clippy                 # 跑 clippy 检查
```

### 常见问题

**Q：第一次用，进度条显示的剩余时间好像不准？**

A：正常。第一次跑的时候，它还不认识你项目里的这些库，只能用一个保守的默认值来猜。**多跑几次**，它把每个库的真实耗时都记下来之后，ETA 就会明显变准。

**Q：它记的数据存在哪？会传出去吗？**

A：存在你自己电脑上的 `~/.cargo/eta-cache.json`（Windows 是 `C:\Users\你的用户名\.cargo\eta-cache.json`）。**不会联网、不会上传**。想清空的话，直接删掉这个文件即可，下次会重新学习。

**Q：为什么有时候开头百分比跳来跳去？**

A：精确的百分比需要 Cargo 的 nightly 版本才能提前拿到「总共有多少个编译单元」。如果你用的是稳定版（大多数人都是），它会一边编译一边数，所以**开头**的数字会有点晃，越往后越稳。这不影响使用。

**Q：我在 CI 里跑，会不会满屏进度条刷屏？**

A：不会。它会自动检测到「这不是交互式终端」，然后改成每 5 秒打一行朴素的日志，不会刷屏。

**Q：编译失败了怎么办？**

A：进度条会正常停下并显示失败。失败那次的耗时**不会**被记进模型——因为失败可能编译到一半就断了，那个时间没有参考价值。

---

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