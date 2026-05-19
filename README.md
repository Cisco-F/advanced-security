# HASM-OpenBMC: STM32 底板管理控制器

一个基于STM32F407ZG的嵌入式底板管理控制系统（BMC），用于远程管理和控制树莓派。支持UART远控、电源管理、USB镜像传输和多种存储后端。

## 🎯 项目概述

HASM-OpenBMC是一个完整的BMC解决方案，部署在STM32微控制器上，用以实现对树莓派的集中式管理和控制。系统通过网络接口与主机通信，支持远程UART终端、电源管理、镜像传输等多项功能。

## 设备需求

- *主机控制端*： Linux/Windows均可
- 主控板：STM32F407ZG
- 受控板：树莓派4B

## 快速开始

配置您的主机以太网IP为192.168.1.77，子网掩码255.255.255.0。您也可以通过修改`hasm-openbmc/src/consts.rs`中的`HOST_IP`常量来调整主机IP地址。

### 接线说明

- STM32 PA10 - 树莓派 TXD
- STM32 PA9 - 树莓派 RXD
- STM32 PB3 - 树莓派 GPIO 17
- STM32 PB4 - 树莓派 GPIO 3


### 烧录运行

使用st-link或DAP调试器连接stm32与您的主机，同时连接网线、USB OTG线。运行命令：
```bash
python python/remote_image_server.py
# 启动另一个终端
cargo run
```
STM32初始化完毕后再给树莓派上电。注意不要给树莓派插tf卡，使树莓派优先通过STM32启动。
可以通过`python python/uart_console.py`连接UART远程终端。

## 其他功能

您可以参考hasm-openbmc/examples目录下的示例代码，了解使用其他USB MSC数据后端及组件初始化方式。


## 📄 许可证

Apache License 2.0

---


