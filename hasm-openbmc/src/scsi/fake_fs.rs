pub const fn build_boot_sector() -> [u8; 512] {
    let mut buf = [0x00; 512];
    buf[0] = 0xEB; buf[1] = 0x3C; buf[2] = 0x90; 
    // "MSDOS5.0"
    buf[3] = 0x4D; buf[4] = 0x53; buf[5] = 0x44; buf[6] = 0x4F; buf[7] = 0x53; buf[8] = 0x35; buf[9] = 0x2E; buf[10] = 0x30;
    
    // 每扇区512字节
    buf[11] = 0x00; buf[12] = 0x02;
    // 每簇2个扇区
    buf[13] = 0x02;
    // 1个保留扇区
    buf[14] = 0x01; buf[15] = 0x00;
    // 2个FAT表
    buf[16] = 0x02;
    // 根目录项 112个
    buf[17] = 0x70; buf[18] = 0x00;
    // ★★总扇区数: 720 个 (0x02D0) = 360KB★★
    buf[19] = 0xD0; buf[20] = 0x02;
    // 介质描述符
    buf[21] = 0xFD;
    // 每个FAT占2个扇区
    buf[22] = 0x02; buf[23] = 0x00;
    // 签名
    buf[38] = 0x29;
    // 卷标: "RUST DISK  "
    buf[43] = b'R'; buf[44] = b'U'; buf[45] = b'S'; buf[46] = b'T'; buf[47] = b' '; 
    buf[48] = b'D'; buf[49] = b'I'; buf[50] = b'S'; buf[51] = b'K'; buf[52] = b' '; buf[53] = b' ';
    // 文件系统: "FAT12   "
    buf[54] = b'F'; buf[55] = b'A'; buf[56] = b'T'; buf[57] = b'1'; buf[58] = b'2'; buf[59] = b' '; buf[60] = b' '; buf[61] = b' ';
    
    // ★★魔法结束标志★★
    buf[510] = 0x55; buf[511] = 0xAA;
    
    buf
}

// 1. FAT 表（告诉系统：文件存放在 2 号簇，并且这是最后一个簇 0xFFF）
pub static FAT_SECTOR: [u8; 512] = {
    let mut buf = [0; 512];
    buf[0] = 0xF0; // 媒体类型
    buf[1] = 0xFF; 
    buf[2] = 0xFF;
    buf[3] = 0xFF; // 2号簇被标记为结束 (EOF)
    buf[4] = 0x0F; // FAT12 需要在后面补一个半字节的 0xF
    buf
};

// 2. 根目录（在这里放我们的文件名！）
pub static ROOT_DIR_SECTOR: [u8; 512] = {
    let mut buf = [0; 512];
    // 前 11 个字节必须是文件名，不足补空格
    buf[0] = b'H'; buf[1] = b'E'; buf[2] = b'L'; buf[3] = b'L'; buf[4] = b'O'; 
    buf[5] = b' '; buf[6] = b' '; buf[7] = b' '; // 空格填满 8 字节文件名
    buf[8] = b'T'; buf[9] = b'X'; buf[10] = b'T'; // 3 字节扩展名
    
    buf[11] = 0x20; // 属性：Archive (普通文件)
    // 12~25 字节是时间，可以留 0
    // 起始簇号 (2号簇) -> 放在偏移 0x1A 和 0x1B，低字节在左
    buf[0x1A] = 0x02; 
    buf[0x1B] = 0x00;
    // 文件大小 (比如 29 个字节) -> 放在 0x1C ~ 0x1F，小端序
    buf[0x1C] = 29; 
    buf
};

// 3. 文件的真正文本内容！(LBA 12 等同于 2号数据簇)
pub static HELLO_DATA_SECTOR: [u8; 512] = {
    let mut buf = [0; 512];
    let text = b"Hello from Rust bare-metal!\r\n";
    let mut i = 0;
    while i < text.len() {
        buf[i] = text[i];
        i += 1;
    }
    buf
};