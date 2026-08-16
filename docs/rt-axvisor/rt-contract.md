# AxVisor 实时性契约（RT Contract）

> 范围：aarch64（qemu-aarch64 / OrangePi 5 Plus），FreeRTOS 类 RTOS guest。
> 配套文档：[验收指标](acceptance-metrics.md) · 改造计划见 `os/axvisor` 与 `virtualization/*` 的 `rt` feature。

本文件定义 AxVisor 为实时 guest 提供的确定性**契约**：哪些是硬保证、哪些是显式不提供、guest 必须遵守什么，以及违约时的行为。

## 1. 分区契约（CPU Partitioning）

当前 RT 模型是 **CPU 分区 + 无 host 时间片抢占**：

- 每个 RT vCPU **独占一个物理核**（VM 配置 `phys_cpu_ids = [N]`，host 侧通过 task cpumask 固定）。
- RT 核上只跑该 vCPU 任务 + idle 任务；**每核 gc 任务被禁用**（`set_gc_disabled_cpu_mask`），管理 shell / 后台任务跑在管理核。gc 禁用的核集合由运行时从**所有 VM 的 `phys_cpu_ids` 推导**（不是独立的板级字段），保证与真实核放置一致。
- vCPU 任务**不允许迁移**；`rt-trim-sysreg` 等优化以“不迁移、不换 VM 切换”为前提（迁移会破坏精简后的 EL1 寄存器保存）。
- 单核约束：RT 核上的 guest 必须是**单 vCPU**（多 vCPU RT guest 启动时报错）。

## 2. 中断 / 定时器契约（passthrough）

- Guest 使用 **EL1 物理定时器 CNTP** + **物理 GIC**，`HCR_EL2.IMO/FMO` 保持清空：guest 中断**直接到达 EL1，不经 hypervisor**。hypervisor 不处于 guest 单次中断延迟路径中。
- **Host tick（CNTHP_EL2）在 guest 运行期间无法到达 EL2**（PPI 被交给 EL1）。因此 host 侧所有定时器/调度工作只在 guest exit 边界执行。
- **guest 必须每 ≤ T（默认 1 ms）至少退出一次**（trap 到 EL2）。健康 RTOS 由其自身的 CNTP tick + WFI 空闲保证。违约 = 可探测故障（可选看门狗，Phase 4）。
- AxVM 定时器轮（timer wheel）在每次 guest exit 时排空并重臂 CNTHP；有界迭代。

## 3. 内存 / MMU 契约

- Guest 运行在 EL1，当前实现假设 **EL1 关 MMU（SCTLR_EL1.M=0）**。该假设在每次 exit 时**复检**（`guest_mmu_off` 标志），仅在持续成立时才允许省略 `tlbi alle1` 等优化——**绝不编译期假设**。
- 二阶段页表（NPT）由 hypervisor 管理；映射变更通过 generation 计数器驱动下一次 guest 入口的延迟/合并 TLB 维护。

## 4. 明确不提供（当前阶段）

- **不提供 host 时间片抢占**：vCPU 任务不会因 host tick 在 guest 运行中被抢占。如需（多 guest 共享核 / host 与 guest 公平调度），见 Phase 6，需切换为 emulated 中断模式或 CNTHP PPI Group-0 路由，属模型变更。
- **不提供虚拟串口**：guest 输出直通物理 UART，与 host 日志共享线路（以前缀区分）。
- **不提供 vGIC / vtimer 模拟**：passthrough 下 guest 直接操作物理 GIC 与 CNTP。

## 5. 违约处理策略

| 违约 | 检测 | 行为 |
|------|------|------|
| guest 超过 T 未退出 | 可选 CNTHP 看门狗（Phase 4） | 记录 `WatchdogMiss`；可暂停 VM 供诊断 |
| vCPU 被迁移 / 多核共享 | 入口断言 last-run 核 == 固定核 | panic/报错，暴露配置错误 |
| guest 使能 MMU 但未告知 | `SCTLR_EL1.M` 每 exit 复检 | 自动退回全量 flush/全量 sysreg 保存 |
| guest 触碰未映射内存 | NPT fault | host 按需映射并 bump generation |

## 6. 目标指标（详见 acceptance-metrics.md）

- guest IRQ→任务延迟（passthrough 下 ≈ 纯 guest 内延迟）
- EL2 exit→entry 周期（host 服务点开销）
- host 任务唤醒延迟
- 最坏 IRQ-off 窗口
