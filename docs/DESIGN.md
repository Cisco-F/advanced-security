# 设计说明

## 目标

HASM-OpenBMC 的目标是在 STM32F407ZG 上实现一个最小可用的 BMC：主机可以远程查看串口、控制树莓派电源，并为树莓派提供 USB 启动介质。

系统优先保证结构清晰和可验证性，不追求完整 Redfish、完整 USB MSC 写入支持或高性能远程存储。

## 总体结构

固件使用 Embassy 异步运行时。主要功能被拆成多个长期运行的 task：

- 网络任务：驱动 Embassy 网络栈。
- UART 控制台任务：在 TCP socket 和 USART1 之间转发数据。
- HTTP 管理任务：提供简化 Redfish 风格接口。
- 电源控制任务：串行处理上电和关机信号。
- USB 任务：处理 USB 枚举和 MSC bulk 传输。
- LED 任务：反映当前电源状态。

## 启动介质路径

默认启动路径为：

```text
主机镜像文件
  -> python/remote_image_server.py
  -> HTTP Range 请求
  -> RemoteBlockDevice
  -> CachedData
  -> SCSI READ(10)
  -> USB MSC
  -> 树莓派
```

STM32 不保存完整镜像，只按扇区请求主机上的镜像数据。这样可以节省 MCU 存储空间，也便于在主机上替换启动镜像。

## 块设备后端

所有存储后端都实现 `BlockDevice`：

- `RemoteBlockDevice`：通过 HTTP Range 从主机读取镜像。
- `TfBlockDevice`：从 TF 卡读取真实块设备。
- `ExampleBlockDevice`：生成一个小型 FAT 示例盘，用于 USB MSC 调试。

`CachedData` 用于缓存相邻扇区，减少树莓派启动时频繁读取元数据造成的网络请求。

## USB MSC 设计

USB MSC 使用 Bulk-Only Transport：

- 主机发送 CBW。
- 固件解析 SCSI 命令。
- 需要数据时读取块设备并通过 bulk IN 返回。
- 最后发送 CSW。

当前实现以只读启动盘为目标，重点支持启动和探测所需的 SCSI 命令，例如 `INQUIRY`、`READ CAPACITY(10)`、`READ(10)` 等。

## 电源控制

PB3 和 PB4 被抽象为两个低电平有效的控制脉冲。HTTP 请求只发送状态信号，真正的 GPIO 操作由 `power_task` 串行执行，避免多个请求同时操作控制脚。

## 管理接口

HTTP 服务只实现项目需要的最小接口：

- `/ping`
- `/redfish/v1`
- `/redfish/v1/Systems`
- `/redfish/v1/Systems/1`
- `/redfish/v1/Systems/1/Actions/ComputerSystem.Reset`

它不是完整 Redfish 实现，主要用于主机脚本和人工调试。

## 取舍

- 不支持 USB 写入：避免树莓派修改主机镜像，保证实验可重复。
- 不引入完整 HTTP/JSON 解析器：减少固件复杂度和内存占用。
- 硬件相关逻辑不做 host 单元测试：通过构建、示例和上板验证覆盖。
- VNC 服务保留为实验功能，不作为主流程依赖。
