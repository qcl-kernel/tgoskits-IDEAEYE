# 构建 FreeRTOS RTOS Guest（`rtos.bin`）

`rtos.bin` 是 QEMU `virt`（AArch64）上的 FreeRTOS + FreeRTOS+TCP 固件，
实现 AXNET/1 TCP Server（192.168.100.3:5000）。FreeRTOS 源码**不放入本仓库**，
构建工程在仓库外（`/home/qwp/freertos-rtos/rtos/`）；这里只存放构建产物和本说明。

## 依赖

```bash
# 交叉编译器（host 上执行，需要 root）
sudo apt-get install -y gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu libgcc-11-dev-arm64-cross
```

## 源码布局（仓库外）

```
freertos-rtos/
├── FreeRTOS-Kernel/      # https://github.com/FreeRTOS/FreeRTOS-Kernel (main)
├── FreeRTOS-Plus-TCP/    # https://github.com/FreeRTOS/FreeRTOS-Plus-TCP (main)
└── rtos/                 # 本工程的 Makefile + BSP + 驱动 + 应用
    ├── Makefile
    ├── include/          # FreeRTOSConfig.h / FreeRTOSIPConfig.h
    ├── bsp/              # boot.S vectors.S uart gic timer link.ld
    ├── drv/              # virtio_net.c (virtio-mmio 驱动)
    ├── netif/            # FreeRTOS+TCP NetworkInterface 胶水
    ├── app/              # axnet1.c + main.c (AXNET/1 server)
    └── bin/              # 输出 rtos.elf / rtos.bin
```

## 构建

```bash
cd freertos-rtos/rtos
make            # 产出 bin/rtos.elf 与 bin/rtos.bin（静态 IP 192.168.100.3，TAP 桥接用）
make clean && make AXNET_DHCP=1   # DHCP 版（SLIRP 无 root 验证用）
make clean      # 清理
```

两种固件网络配置：

| 版本            | 网络       | 用途                      |
| --------------- | ---------- | ------------------------- |
| `make`（默认）   | 静态 IP    | TAP/br0 桥接（方案主线）   |
| `make AXNET_DHCP=1` | DHCP    | SLIRP 无 root 验证        |

## 产出拷贝回仓库

```bash
# 静态版（TAP 桥接）
cp bin/rtos.bin  <repo>/tools/starry-rtos-net/rtos/rtos.bin
cp bin/rtos.elf  <repo>/tools/starry-rtos-net/rtos/rtos.elf

# DHCP 版（SLIRP 验证，需先 make clean && make AXNET_DHCP=1）
cp bin/rtos.bin  <repo>/tools/starry-rtos-net/rtos/rtos-dhcp.bin
cp bin/rtos.elf  <repo>/tools/starry-rtos-net/rtos/rtos-dhcp.elf
```

## 单元级验证

AXNET/1 协议逻辑可用主机 gcc 直接测（不依赖交叉编译器）：

```bash
cd freertos-rtos/rtos
make hosttest      # 编译并运行 tests/axnet1_host_test.c
```

## QEMU 快速验证（不需要 root / TAP）

```bash
# 终端 1：以 user 网络 + hostfwd 启动 RTOS guest
tools/starry-rtos-net/host/rtos-standalone.sh

# 终端 2：从 host 验证服务器全行为（握手/心跳/周期 STATUS/坏帧 ERROR）
cargo run -p rtos-tester -- --server 127.0.0.1 --port 5000
```
