use crate::block::BlockDevice;


pub static BOOT_SECTOR: [u8; 512] = build_boot_sector();

pub struct ExampleBlockDevice;

impl BlockDevice for ExampleBlockDevice {
    async fn read_block(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        match lba {
            0 => buf.copy_from_slice(&BOOT_SECTOR[..Self::BLOCK_SIZE as usize]),
            1 | 257 => buf.copy_from_slice(&FAT_SECTOR[..Self::BLOCK_SIZE as usize]),
            513 => buf.copy_from_slice(&ROOT_DIR_SECTOR[..Self::BLOCK_SIZE as usize]),
            545 => buf.copy_from_slice(&HELLO_DATA_SECTOR[..Self::BLOCK_SIZE as usize]),
            _ => buf.fill(0), // 其他未使用的块全填充为0，模拟一个干净的空盘
        }

        Ok(())
    }

    async fn read_blocks(&mut self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        let blocks_to_read = (buf.len() as u32 + Self::BLOCK_SIZE - 1) / Self::BLOCK_SIZE;
        for i in 0..blocks_to_read {
            let block_lba = lba + i;
            let block_offset = (i * Self::BLOCK_SIZE) as usize;
            let block_buf = &mut buf[block_offset..block_offset + Self::BLOCK_SIZE as usize];
            self.read_block(block_lba, block_buf).await?;
        }
        Ok(())
    }
}

pub const fn build_boot_sector() -> [u8; 512] {
    let mut buf = [0x00; 512];
    buf[0] = 0xEB; buf[1] = 0x3C; buf[2] = 0x90; 
    // "MSDOS5.0"
    buf[3] = b'M'; buf[4] = b'S'; buf[5] = b'D'; buf[6] = b'O'; buf[7] = b'S'; 
    buf[8] = b'5'; buf[9] = b'.'; buf[10] = b'0';
    
    // 【修改点1】每扇区512字节
    buf[11] = 0x00; buf[12] = 0x02;
    // 【修改点2】FAT16为了容纳256M，必须加大簇。这里设为：每簇16个扇区 (8KB)
    buf[13] = 0x10;
    // 1个保留扇区 (Boot Sector 自身)
    buf[14] = 0x01; buf[15] = 0x00;
    // 2个FAT表
    buf[16] = 0x02;
    // 【修改点3】根目录项 512个 (标准硬盘格式，占用 32 个扇区)
    buf[17] = 0x00; buf[18] = 0x02;
    // 【修改点4】16位总扇区数：因为 524,288 大于 65535，所以这里必须清零！
    buf[19] = 0x00; buf[20] = 0x00;
    // 【修改点5】介质描述符：0xF8 代表硬盘 (之前 0xFD 是软盘)
    buf[21] = 0xF8;
    // 【修改点6】每个FAT占用的扇区数：256个扇区 (能装得下所有FAT项)
    buf[22] = 0x00; buf[23] = 0x01; 
    
    // 省略 CHS 几何参数 (保留0即可)

    // 【修改点7】32位总扇区数：放在偏移 0x20_处。524,288 = 0x00080000
    buf[32] = 0x00; buf[33] = 0x00; buf[34] = 0x08; buf[35] = 0x00;

    // 驱动器号：0x80 (固定磁盘)
    buf[36] = 0x80;
    // 签名
    buf[38] = 0x29;
    // 卷标: "RUST DISK  "
    buf[43] = b'R'; buf[44] = b'U'; buf[45] = b'S'; buf[46] = b'T'; buf[47] = b' '; 
    buf[48] = b'D'; buf[49] = b'I'; buf[50] = b'S'; buf[51] = b'K'; buf[52] = b' '; buf[53] = b' ';
    // 【修改点8】文件系统: 修改为 "FAT16   "
    buf[54] = b'F'; buf[55] = b'A'; buf[56] = b'T'; buf[57] = b'1'; buf[58] = b'6'; 
    buf[59] = b' '; buf[60] = b' '; buf[61] = b' ';
    
    // 魔法结束标志
    buf[510] = 0x55; buf[511] = 0xAA;
    
    buf
}

// 1. FAT 表（FAT16 每个条目占用整整 2 个字节，小端序）
pub static FAT_SECTOR: [u8; 512] = {
    let mut buf = [0; 512];
    // 0 号簇：媒体类型 (0xF8) + 结束符 (0xFF)
    buf[0] = 0xF8; buf[1] = 0xFF; 
    // 1 号簇：保留给系统标记 (0xFFFF)
    buf[2] = 0xFF; buf[3] = 0xFF;
    // 2 号簇（我们的文件所在的簇）：直接标记为结束 (0xFFFF，表示这是链表的最后一截)
    buf[4] = 0xFF; buf[5] = 0xFF; 
    buf
};

// 2. 根目录（没有任何改变！文件仍然指向 2 号簇）
pub static ROOT_DIR_SECTOR: [u8; 512] = {
    let mut buf = [0; 512];
    buf[0] = b'H'; buf[1] = b'E'; buf[2] = b'L'; buf[3] = b'L'; buf[4] = b'O'; 
    buf[5] = b' '; buf[6] = b' '; buf[7] = b' '; 
    buf[8] = b'T'; buf[9] = b'X'; buf[10] = b'T'; 
    
    buf[11] = 0x20; 
    
    // 起始簇号 (2号簇) -> FAT16 也是放在 0x1A
    buf[0x1A] = 0x02; 
    buf[0x1B] = 0x00;
    // 文件大小 (比如 29 个字节)
    buf[0x1C] = 29; 
    buf
};

// 3. 文件的文本内容（也没有改变）
pub static HELLO_DATA_SECTOR: [u8; 512] = {
    let mut buf = [0; 512];
    let text = b"Welcome to 256MB Rust Disk!\r\n";
    let mut i = 0;
    while i < text.len() {
        buf[i] = text[i];
        i += 1;
    }
    buf
};