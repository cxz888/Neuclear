# kernel

## 目录结构

本项目采取 workspace 方式，包含多个 crate。

所有的代码包都放在 `crates` 目录下，这也是 rust 项目使用 workspace 时常采取的结构

### drivers

设备驱动和缓存。

目前只有对块设备的抽象。

具体的实现则只有对 virtio 的实现。

### fat32

只是包一层第三方库 fatfs 来实现接口

### filesystem

文件系统相关

### major

内核的主体部分，也是这里唯一一个二进制包，编译得到的结果就是内核本体

主要包含难以拆分的进程/线程抽象、trap 处理和 syscall。

### memory

虚拟地址、帧分配、内核堆等。

### signal

信号机制相关。

### utils

各种通用的组件，包括 print、log、time、config 等等

### vfs

virtual file system.

理想情况下，希望每增加一种操作系统的支持，就为其实现 vfs 的接口。内核那边不太需要关心这种改变。

不过实际上现在内核那里还是写死了用的 fat32。
