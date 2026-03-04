# 硬件说明
使用野火霸天虎（STM32F407ZG）开发板，其搭载LAN8720A以太网PHY芯片及ESP8266 WiFi模块。

## LAN8720A接线说明
| LAN8720A引脚 | STM32F407ZG引脚 | 
|--------------|-----------------|
| REFCLK | PA1 |
| MDIO | PA2 |
| MDC | PC1 |
| CSR_DV | PA7 |
| RXD0 | PC4 |
| RXD1 | PC5 |
| TXD0 | PG13 |
| TXD1 | PG14 |
| TXEN | PG11 |

## ESP8266接线说明

# 资源树
```
/redfish/v1
├── Systems
│   └── 1
│       └── Actions
│           └── ComputerSystem.Reset
├── Chassis
│   └── 1
└── Managers
    └── 1
```

## 资源树说明
### 健康测试
```
GET /ping
```
测试服务是否正常运行

### Service Root
```
GET /redfish/v1
```
Redfish入口，提供资源目录，不包含设备行为

### Systems集合
```
GET /redfish/v1/Systems
```
返回系统资源列表

### ComputerSystem资源
```
GET /redfish/v1/Systems/1
```

### 主机电源控制
```
POST /redfish/v1/Systems/1/Actions/ComputerSystem.Reset
```
请求格式：
```json
{
    "ResetType": "On"
}
```

支持的 ResetType：
| ResetType | 描述 |
|-----------|------|
| On | 立即上电 |
| ForceOff | 立即断电 |