# freertos-rt-probe

一个极简的 FreeRTOS 风格 **RT-probe guest**，用于测量 AxVisor hypervisor 的确定性/最坏响应特性（aarch64，EL1，关 MMU）。它不是完整 FreeRTOS 内核，而是复刻 FreeRTOS AArch64 移植会用到的基础设施：

- EL1 异常向量表（`VBAR_EL1 @ 0x4000_0000`）
- EL1 物理定时器 CNTP 作为周期 tick（1 ms）
- 物理 GICv3（passthrough）的 CNTP PPI（GIC ID 30）
- 极简协作式任务模型：CNTP IRQ → 高优先级 reporter 任务
- 固定 GPA 的共享 RT-stats 页（host 可用 `read_from_guest` 读回）

## 内存布局

| 地址 | 内容 |
|------|------|
| `0x4000_0000` | 异常向量（= kernel 加载基址） |
| `0x4000_1000` | `_start`（VM 配置 `entry_point`） |
| `0x4000_2000`+ | .text / .rodata / .data / .bss / .stack |
| `0x4070_0000` | 共享 `RtStats` 页 |
| `0x4080_0000` | guest RAM 末端（128 MiB） |
| `0x0800_0000` | GICD，`0x080A_0000` GICR(CPU0)，`0x0900_0000` PL011 |

## 构建

```bash
./build.sh   # cargo build --release + objcopy 出 freertos-rt.bin
```

需要 repo `rust-toolchain.toml` 的 nightly 工具链（含 `llvm-tools`）。

## 接入 AxVisor 运行

```bash
./setup.sh   # 构建 guest 并生成 os/axvisor/tmp/vmconfigs/freertos-rt-smp1.generated.toml

cd <repo>
cargo xtask axvisor qemu \
  --config configs/board/qemu-aarch64-rt.toml \
  --qemu-config .github/workflows/qemu-aarch64-rt.toml \
  --vmconfigs os/axvisor/tmp/vmconfigs/freertos-rt-smp1.generated.toml
```

guest 每 2000 tick（约 2 s）通过直通 UART 打印一行：

```
[RT] ticks=... samples=... irq2task min=... avg=... max=... handler_max=... exits=...
```

`irq2task` 为 IRQ→任务响应延迟（周期数）；host 侧测量见 `docs/rt-axvisor`（Phase 1，`rt-instrument`）。

## 换成真 FreeRTOS

drop-in：保持加载基址 `0x4000_0000`、向量在 `0x4000_0000`、入口 `0x4000_1000`、单 vCPU passthrough，把 `freertos-rt.bin` 换成 FreeRTOS 编译产物即可。
