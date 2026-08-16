# RT 验收指标（Acceptance Metrics）

> 范围：aarch64 · FreeRTOS 类 RTOS guest。所有指标在改造前先建立**基线**，逐期优化后重测对比。
> 相关：[RT 契约](rt-contract.md)。

## 1. 指标定义

| 指标 | 定义 | 采样点 | 单位 |
|------|------|--------|------|
| **guest IRQ→任务延迟** | CNTP 定时器 IRQ 到达 → 高优先级任务开始执行 | guest 内 recorder（`guests/freertos-rt`） | 周期 / µs，报 min/avg/p99/max |
| **guest IRQ handler 执行时间** | IRQ 入口 → handler 返回（EOI 前） | guest 内 recorder | 周期 |
| **EL2 exit→entry 周期** | `ArmVcpu::run` 进入 guest → 下一次 `eret` 重入前的完整 exit 处理 | host 侧 `arm_vcpu` 埋点（`rt-instrument`） | 周期，p50/p99/max |
| **exit reason 分布** | 各 `ArmVmExit` 变体计数 | host 侧 `vmexit_handler` | 计数 |
| **host 任务唤醒延迟** | `queue_interrupt`/`send_ipi` → vCPU 任务再调度执行 | host 侧 `runtime/vcpus.rs` | µs |
| **最坏 IRQ-off 窗口** | 单次最长 IRQ 屏蔽连续区间 | host 侧 `SpinNoIrq` / guest 入口埋点 | µs |

## 2. 挑衅场景（provocations）

基线需要同时测“安静”与“挑衅”两种状态，覆盖最坏情况：

1. **安静**：guest 仅 CNTP tick + WFI 空闲，无其它 exit 源。
2. **周期 hvc**：guest 每 N tick 发一次 `hvc`（host 处理一个已知 hypercall），制造稳定 exit 负载。
3. **NPT 缺页风暴**：guest 依次触碰新页面（128 MB 区域），host 反复处理 NPT fault。
4. **并发 IRQ + host 服务**：guest 定时器到期时恰好 host 正在做 bulk `map_region`/`unmap_region`（最坏 IRQ→任务延迟）。

## 3. 基线与对比流程

```
cargo xtask axvisor qemu \
  --config configs/board/qemu-aarch64-rt.toml \
  --qemu-config .github/workflows/qemu-aarch64-rt.toml \
  --vmconfigs os/axvisor/tmp/vmconfigs/freertos-rt-smp1.generated.toml
```

1. guest 跑 demo 任务并打印 `RT-STATS ...`（IRQ→任务延迟等）。
2. host 在 VM 停止时打印 `EXIT_STATS`、exit 周期统计、唤醒延迟（Phase 1 埋点）。
3. 每期结束后重跑，与前一期基线 diff 六个指标。

## 4. 当前基线（占位，Phase 1 填充）

> 下表在 Phase 1（`rt-instrument`）落地后填入真实数字。改造前后对比以本表为准。

| 指标 | min | avg | p99 | max |
|------|-----|-----|-----|-----|
| guest IRQ→任务延迟 (µs) | — | — | — | — |
| EL2 exit→entry (cycles) | — | — | — | — |
| host 唤醒延迟 (µs) | — | — | — | — |
| 最坏 IRQ-off (µs) | — | — | — | — |

## 5. RT feature 开关（Linux 验证时对照）

| feature（axvisor 聚合 `rt`） | 阶段 | 作用 | 验证重点 |
|------|------|------|---------|
| `axvm/rt-instrument` | 1 | exit/entry 周期 + 原因计数 + 唤醒延迟埋点 | `RT-STATS:` dump 正确 |
| `axvm/rt-partition` | 2 | 协作式 yield、gc 关核、单 vCPU/核校验 | CPU1 无 gc；guest 时序不变 |
| `axvm/rt-cond-flush` | 3a | 条件化 `ic iallu; tlbi`（默认保守） | entry_cycles avg/max 显著下降 |
| `axvm/rt-trim-sysreg` | 3b | sysreg 精简（passthrough 才激活） | entry/exit 周期再降；Linux guest 回归 |
| `axvm/rt-timer-service` | 4 | 每次 exit 排空 wheel + 有界迭代 | host 虚拟定时器有界 jitter |
| `axvm/rt-lock-opt` | 5b | guest 内存访问先翻译后拷贝 | IRQ-off 窗口缩小；无正确性回归 |
| `axvisor/rt-preempt` | 6 | sched-rr + CNTHP Group-0 + HCR FMO + EL2 FIQ | **最高风险**：guest 用 IRQ 不用 FIQ；host tick 能抢占；ICC_IAR0 应答需验证 |

`rt` = 除 `rt-preempt` 外的全部（Phase 0-5）。`rt-preempt` 需单独启用。

## 6. 通过标准

- 各指标 `max`/`p99` 相比前一阶段基线**不劣化**（功能回归），且优化阶段目标项**显著改善**。
- guest 引导、tick、hvc、NPT 缺页在每期后功能正确。
- Linux guest（MMU-on）在关闭 `rt-trim-sysreg` 下通过现有 qemu-aarch64 冒烟测试，无退化。
