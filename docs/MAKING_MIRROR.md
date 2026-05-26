# 自定义镜像制作

本文说明如何制作一个可被 HASM-OpenBMC 远程提供的启动镜像。推荐使用 Alpine Linux，体积小，启动文件结构也比较简单。

树莓派通常不能直接从单个 ISO 或普通文件启动。HASM-OpenBMC 的远程启动路径希望把启动介质作为一个整体文件传输，所以需要把启动分区打包进一个 `.img` 文件中。

## 准备启动文件

先准备一份适合树莓派的 Alpine 启动文件。可以从 Alpine 官网或树莓派相关镜像获取标准启动包。

假设解压后的启动文件位于：

```text
alpine_img/
```

后续会把这个目录中的文件复制到 `.img` 镜像的 FAT32 分区中。

## 创建空镜像

创建一个 200 MiB 的空镜像文件：

```bash
dd if=/dev/zero of=alpine.img bs=1M count=200
```

如果系统文件较多，可以适当增大 `count`。

## 写入分区表

创建 MBR 分区表，并从 `1MiB` 开始建立一个 FAT32 分区：

```bash
parted -s alpine.img mklabel msdos
parted -s alpine.img mkpart primary fat32 1MiB 100%
```

这一步只是在镜像开头写入 MBR 分区表。分区表声明 FAT32 分区从 `1MiB` 处开始，FAT32 文件系统将在后续步骤进行初始化。

## 挂载镜像

把镜像绑定为 loop 设备：

```bash
sudo losetup -fP alpine.img
losetup -a
```

假设输出中显示镜像被绑定到 `/dev/loop0`，那么分区设备通常是：

```text
/dev/loop0p1
```

如果你的机器显示的是 `/dev/loop1` 或其它序号，后续命令要对应替换。

## 格式化并挂载分区

```bash
sudo mkfs.vfat -F 32 /dev/loop0p1
sudo mkdir -p /mnt/my_alpine
sudo mount /dev/loop0p1 /mnt/my_alpine
```

## 复制启动文件

把准备好的启动文件复制到 FAT32 分区中：

```bash
sudo cp -r alpine_img/* /mnt/my_alpine
sync
```

确认文件已经写入后再卸载。

## 卸载并释放 loop 设备

```bash
sudo umount /mnt/my_alpine
sudo losetup -d /dev/loop0
```

此时 `alpine.img` 就是可以交给主机侧镜像服务器使用的镜像文件。

## 放入项目默认位置

HASM-OpenBMC 默认读取：

```text
img/raspi_recover.img
```

可以这样放置：

```bash
mkdir -p img
cp alpine.img img/raspi_recover.img
```

然后启动镜像服务器：

```bash
python python/remote_image_server.py
```

也可以直接指定镜像路径：

```bash
python python/remote_image_server.py --img alpine.img
```

## 可选：烧录到 TF 卡

如果需要验证镜像本身，也可以用 Rufus、`dd` 等工具把 `alpine.img` 烧录到 TF 卡，再让树莓派从 TF 卡启动。这样可以先排除镜像内容问题，再调试 HASM-OpenBMC 的 USB MSC 启动链路。
