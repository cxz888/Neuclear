# Neuclear

基于 rCore，兼容 Linux，用 Rust 编写的操作系统。

名称化用自 Nuclear 即原子核，与内核 (kernel) 虽然在英文上没什么关联，但至少都带个核不是（）

由于开发人员都来自东北大学 (Neu)，因此化用为 Neuclear 作为队伍名称与系统名称

项目文档见 doc 目录。

## VSCode 扩展建议

- Even Better TOML
- crates
- Error Lens
- C/C++（调试用）
- RISC-V Support
- rust-analyzer
- AutoCorrect（请尽量注意中英文之间加空格隔开，该扩展就是自动做这个的）
- todo tree（用于查看项目中的 TODO/FIXME/NOTE 请尽量下载）

## 可以参考的资料

<https://gitee.com/LoanCold/ultraos_backup>

<https://github.com/equation314/nimbos>

<https://gitlab.eduxiji.net/dh2zz/oskernel2022>

<https://tooldiy.ry.rs/firmware/RustSBI/>

## 项目结构说明

### .vscode

vscode 调试用

### bintool

放一些调试用的工具，像是探查 elf 内容，打包 elf 之类的。

常常也会在此目录下进行二进制文件的反汇编等工作（通过 rust-objdump）

### deps

一些第三方库，可能会需要做出修改来适应本项目的需求

- rust-fatfs 库，做了点细微的修改
- buddy_system_allocator 库，做了些小修改

### doc

项目的一些文档。分模块讲解了 Neuclear 的整体设计和实现方式。

### kernel

内核部分，会编译得到内核的二进制文件

### res

参考资料；系统镜像；BootLoader 之类的。总之是一些相关的资源

### user

rCore 的残留，不过也可以拿来做测试用，所以不急着删。

## 运行方式

根目录下 `make run`。

如希望带有日志，则设置环境变量 `LOG=DEBUG`。

## 调试方式

1. gdb 命令行式，操作更精细，当然也更麻烦
   1. 打开两个终端，工作目录都是 Neuclear 目录
   2. 在一个终端 `make dbg`
   3. 在另一个终端 `make dbg-listener`
2. vscode 交互式，方便点
   1. 终端中运行 `make dbg`
   2. vscode 中按 F5

由于虚拟映射的变换，断点的打法是有技巧的，以 gdb 方式为例：

1. 操作系统刚刚启动，此时起始点是在 linker 脚本里的地址，也即 0x80200000，所以先 `break *0x80200000`，然后 continue 过去。
2. 启动后会加载临时页表，加载后才可以直接给 `rust_main` 打断点，因此先来个 `stepi 15`
3. 此时高地址载入页表，已经可以用 `break rust_main` 了。
4. 另外，因为很快页表会再次变化，所以低地址的断点会无效，记得 `d 1` 删掉第一个断点

其实 1、2 步可以合并，最初就直接 `break *0x8020001a`，然后直接 continue 过去，就可以进行第 3 步了。

vscode 方式也差不多，要手动在调试窗口加个 `break *0x8020001a` 的断点，运行到那里用鼠标点击打断点才有效。

## Todo List

- [x] 页表机制要换。目前是 rCore 式的双页表，通过跳板进行 trap 处理。但实践下来感觉造成的问题更多，切成 linux 或者 windows 那样的最好。这个工作越早进行越好，因为影响面比较广。
- [ ] Cow 虚拟页
- [ ] 页面置换
- [ ] 信号机制。目前还没有具体的处理。
- [ ] 标记 `unsafe`。这里的阻碍是我不太敢对 `unsafe` 乱下手，可能还得花时间看死灵书
- [ ] 多核启动
- [ ] 需要探索的：如何做内核 profile、页表中的 ASID、D/A/G 位

## 注意事项

- rust-analyzer 在 riscv target 下可能有误报 can't find crate for `test`，解决它需要设置 `rust-analyzer.check.allTargets` 为 false
