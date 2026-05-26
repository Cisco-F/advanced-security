# 测试说明

## 构建检查

检查主固件：

```bash
cargo build
```

检查示例：

```bash
cargo build --examples
```

这两项可以发现 Rust 类型错误、模块引用错误，以及示例是否仍能独立编译。

## 主机侧脚本检查

```bash
python -m py_compile python/remote_image_server.py python/uart_console.py
```

用于确认 Python 脚本没有语法错误。

## 上板验证项目

以下功能依赖硬件，不能只靠 PC 单元测试判断：

- 以太网链路是否正常。
- 主机是否能访问 HTTP 管理接口。
- UART 控制台是否能看到树莓派启动日志。
- PB3/PB4 是否能正确触发电源动作。
- USB MSC 是否能被树莓派识别为启动盘。
- TF 卡后端是否能完成初始化和读块。

建议按以下顺序验证：

1. 运行 `blinky`，确认下载和日志输出正常。
2. 运行 `ethernet`，确认链路状态正常。
3. 运行 `uart_console`，确认串口接线正确。
4. 运行 `power`，确认电源控制有效。
5. 运行主固件，验证远程镜像启动路径。


