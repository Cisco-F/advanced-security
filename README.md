# HASM-OpenBMC

HASM-OpenBMC 是一个运行在 STM32F407ZG 上的简易底板管理控制器，用于远程管理和控制树莓派。它提供串口控制台、电源控制，以及通过模拟为 USB Mass Storage 设备向树莓派提供启动镜像的能力。

## 功能

- UART 远程控制台：通过 TCP 连接树莓派串口。
- 电源控制：通过 GPIO 模拟上电和关机控制。
- 虚拟 USB 启动盘：STM32 作为 USB MSC 设备，向树莓派提供块设备。
- 多种块设备后端：远程镜像、TF 卡、示例 FAT 文件系统。
- 简化 Redfish 风格 HTTP 接口：用于查询和控制电源状态。

## 硬件需求

- 主控板：STM32F407ZG
- 被控设备：树莓派 4B
- 调试器：ST-Link 或 DAPLink
- 连接线：以太网线、USB OTG 线、串口/控制 GPIO 线
- 主机：Linux 或 Windows

## 默认网络配置

默认配置适用于主机与 STM32 直连或处于同一局域网的场景：

- STM32 BMC：`192.168.10.2`
- 主机镜像服务器：`192.168.10.1`
- 子网掩码：`255.255.255.0`

## 接线

| STM32 引脚 | 连接到树莓派 | 用途 |
| --- | --- | --- |
| PA10 | TXD | 串口接收 |
| PA9 | RXD | 串口发送 |
| PB3 | GPIO 17 | 电源关断控制 |
| PB4 | GPIO 3 | 上电控制 |
| USB OTG FS | USB | 虚拟启动盘 |
| Ethernet | 主机网络 | 镜像传输和管理接口 |

## 准备启动镜像

主机侧镜像服务器默认读取：

```text
img/raspi_recover.img
```

请先准备好镜像并放到该位置：

```bash
cd hasm-openharmony
mkdir -p img
cp /path/to/your/image.img img/raspi_recover.img
```

也可以在启动服务器时通过 `--img` 指定其它路径：

```bash
python python/remote_image_server.py --img /path/to/your/image.img
```

如果需要自己制作 Alpine 或其它系统镜像，参考 [自定义镜像制作](docs/MAKING_MIRROR.md)。

## 快速运行

先将主机 IP 配置为 `192.168.10.1/24`，连接以太网、USB OTG 和调试器。
> [!TIP]
> 如需修改地址，调整 [hasm-openbmc/src/consts.rs](hasm-openbmc/src/consts.rs) 中的常量。

启动主机侧镜像服务器：

```bash
python python/remote_image_server.py
```

另开一个终端烧录并运行固件：

```bash
cargo run
```

STM32 初始化完成后再给树莓派上电。若希望树莓派从 STM32 模拟的 USB 启动盘启动，请不要插入树莓派 TF 卡。

连接远程串口控制台：

```bash
python python/uart_console.py
```

## 示例

[hasm-openbmc/examples](hasm-openbmc/examples) 中包含独立示例：

- `blinky.rs`：LED 和基础运行环境测试。
- `ethernet.rs`：以太网链路测试。
- `uart_console.rs`：远程串口控制台。
- `power.rs`：HTTP 电源控制。
- `example_fs.rs`：内置 FAT 示例盘。
- `tf.rs`：TF 卡作为 USB MSC 后端。
- `vnc.rs`：实验性 VNC/RFB 服务。

运行示例：

```bash
cargo run --example ethernet
```

## 文档

- [设计说明](docs/DESIGN.md)
- [测试说明](docs/TESTING.md)
- [自定义镜像制作](docs/MAKING_MIRROR.md)

## 许可证

Apache License 2.0
