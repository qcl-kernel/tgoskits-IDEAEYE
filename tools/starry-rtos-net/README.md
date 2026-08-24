# StarryOS ↔ FreeRTOS：QEMU 双 Guest IP/TCP 通信（AXNET/1）

本目录实现 `solution.md` 的 QEMU 方案：**两个 QEMU Guest（StarryOS + FreeRTOS）
通过 virtio-net → tap0/tap1 → Linux bridge 互联，应用层跑 AXNET/1 协议**。

```text
                        Host Linux
┌───────────────────────────────────────────────────────┐
│                      br0 (192.168.100.1/24)           │
│                          │                            │
│                     ┌────┴────┐                       │
│                   tap0        tap1                    │
│                     │           │                     │
│        ┌────────────┘           └────────────┐        │
│   QEMU #1  StarryOS                    QEMU #2  FreeRTOS│
│   192.168.100.2                    192.168.100.3     │
│   virtio-net-pci                   virtio-mmio      │
│        └─────────── TCP :5000 ─────────────┘        │
└───────────────────────────────────────────────────────┘
```

业务数据只走 TCP/IP。VirtIO 队列只是虚拟网卡实现机制，不是应用协议主通道，
因此满足题目“共享内存 / HyperCall / 裸 MMIO 不得作为主数据通道”。

## 目录结构

```text
tools/starry-rtos-net/
├── Cargo.toml                  # 独立 Cargo workspace（不参与仓库主 workspace）
├── README.md
├── rtos/
│   ├── BUILD.md                # 如何从仓库外的 FreeRTOS 工程构建 rtos.bin
│   └── rtos.bin                # 构建产物（FreeRTOS + FreeRTOS+TCP AXNET/1 server）
├── host/
│   ├── setup-bridge.sh         # 建 br0 + tap0 + tap1
│   ├── teardown-bridge.sh
│   ├── run-rtos.sh             # 启动 FreeRTOS guest（tap1）
│   ├── run-starry.sh           # 启动 StarryOS guest（tap0）
│   ├── rtos-standalone.sh      # 无 root 验证：user 网络 + hostfwd :5000
│   ├── kill-rtos.sh            # 故障注入：杀 RTOS QEMU
│   ├── tap-toggle.sh           # 故障注入：tap1 down/up
│   ├── demo.sh                 # 一键编排
│   └── starry-qemu-tap-x86_64.toml   # StarryOS QEMU TAP 运行配置
├── crates/
│   ├── axnet1/                 # AXNET/1 协议（Rust，与 C 端字节级兼容）
│   ├── starry-client/          # StarryOS 侧 AXNET/1 TCP 客户端
│   └── rtos-tester/            # host 侧服务器验证器
└── tests/integration/          # host loopback 集成测试（协议+客户端状态机）
```

## 固定网络参数

| 项       | StarryOS            | FreeRTOS            |
| -------- | ------------------- | ------------------- |
| MAC      | `02:00:00:00:01:02` | `02:00:00:00:01:03` |
| IP       | `192.168.100.2`     | `192.168.100.3`     |
| Mask     | `255.255.255.0`     | `255.255.255.0`     |
| TCP Port | 5000                | 5000                |
| Gateway  | 不需要               | 不需要               |

## AXNET/1 协议（C/Rust 双实现，黄金帧字节级一致）

```text
Magic 0xA501 | Ver 1 | MsgType | Flags u16 | PayloadLen u32
Sequence u32 | TimestampUs u64 | ErrorCode u32 | Payload | CRC32
```

MsgType：`CONTROL 0x01 / CONTROL_ACK 0x02 / STATUS 0x03 / ERROR 0x04 / HEARTBEAT 0x05`
全部字段大端；CRC32 覆盖除尾 4 字节外的整个帧。TCP 字节流按帧解析（`FrameReader`）。

## 快速开始

### 1. 构建并验证（无需 root）

```bash
# 构建两个 RTOS guest 变体（需要 aarch64-linux-gnu-gcc，见 rtos/BUILD.md）
cd <仓库外>/freertos-rtos/rtos
make                                  # 静态版 → 拷贝 rtos.bin / rtos.elf
make clean && make AXNET_DHCP=1       # DHCP 版 → 拷贝 rtos-dhcp.bin / rtos-dhcp.elf

# 主机集成测试（协议 + 客户端状态机：心跳/重连/超时恢复/分帧）
cd tools/starry-rtos-net && cargo test --workspace

# 无 root 验证 FreeRTOS server（QEMU + SLIRP + hostfwd，guest 走 DHCP）
./host/rtos-standalone.sh             # 终端 1
cargo run -p rtos-tester              # 终端 2：握手/心跳/周期STATUS/坏帧ERROR 全验证
cargo run -p starry-client -- --server 127.0.0.1 --port 5000 --requests 100  # RTT/吞吐统计
```

### 2. 完整双 QEMU 演示（需要 root 建 TAP/br0）

```bash
sudo ./host/setup-bridge.sh
./host/run-rtos.sh              # 终端 1：FreeRTOS guest（前台，打印 AXNET1_SERVER_READY）
./host/run-starry.sh            # 终端 2：StarryOS guest（先注入 client，见下）
# StarryOS 内：
#   /usr/bin/starry-client --server 192.168.100.3 --port 5000 --requests 10000 --payload 1024
```

### 3. StarryOS 侧 client 注入

`starry-client` 是纯 Rust 静态 musl 程序，构建后拷入 Alpine rootfs：

```bash
rustup target add x86_64-unknown-linux-musl
cd tools/starry-rtos-net && cargo build -p starry-client --target x86_64-unknown-linux-musl --release

# 注入 rootfs（rootfs 由 `cargo xtask starry rootfs` 获取，路径见 run-starry.sh）
sudo mkdir -p /mnt/starry && sudo mount -o loop tmp/axbuild/rootfs/rootfs-x86_64-alpine.img /mnt/starry
sudo cp target/x86_64-unknown-linux-musl/release/starry-client /mnt/starry/usr/bin/
sudo umount /mnt/starry
```

`starry-client` 同时是主机可运行的：连不上 server 时它以指数退避重连并打印统计，
可作为“断连重连 / 超时恢复”的可观测演示。

## 故障注入测试

| 测试                 | 操作                            | 预期                                       |
| -------------------- | ------------------------------- | ------------------------------------------ |
| 正常 10K 请求          | `--requests 10000`              | `RESULT=PASS`，成功率 100%                |
| 杀 RTOS QEMU          | `./host/kill-rtos.sh` 再 `run-rtos.sh` | client 检测断线→退避重连→恢复业务       |
| tap1 down/up          | `./host/tap-toggle.sh tap1 5`   | 流量中断→client 超时→重连恢复             |

## 测量项（对应 solution.md §14/§15）

`starry-client` 汇总输出：

```text
[starry-client] sent=... acked=... lost=... errors=... timeouts=... reconnects=...
[starry-client] avg_rtt_us=... p50_rtt_us=... p95_rtt_us=... p99_rtt_us=... max_rtt_us=...
[starry-client] throughput_bytes=... throughput_us=... throughput_mbps=...
```

- RTT：每个 CONTROL 请求的发送→ACK 间隔（仅统计 Payload 的有效应用吞吐）。
- 吞吐：`PayloadBytes / 传输耗时`，不含 Ethernet/IP/TCP/AXNET 头与 CRC。

## 迁移到 AxVisor（第二阶段）

两边的网络参数（IP/MAC/端口）都在编译期常量里（FreeRTOS 侧 `app/main.c` 的
`AXNET1_IP*`；StarryOS 侧命令行参数）。迁到 AxVisor 时，只需把 QEMU 的
`virtio-net-device` 换成 AxVisor 的虚拟网卡后端，Guest 内应用与协议栈**无需改动**。
