# Granite

## 规范

加载器加载 ELF 可执行文件时，`argc`、`argv`、参数、环境变量、`auxv`、如何排布是有具体要求的。

可以参考：

- <http://articles.manugarg.com/aboutelfauxiliaryvectors.html>
- <https://gitlab.eduxiji.net/scPointer/maturin/-/blob/master/kernel/src/loaders/init_info.rs#L27>
- <http://www.lenky.info/archives/2013/02/2203>

res/abi386-4.pdf 中的 Figure 3-31 也有比较粗略的描述。

`auxv` 即辅助向量，可以参考 <https://blog.csdn.net/choumin/article/details/111385498>

注意，辅助向量要尽早完成，因为 `PAGE_SIZE` 等是要参考它的。

> 如果遇到 mmap 时 len==0 的情况，可能是 auxv 没有处理的原因。
> 在 Linux 现行规范中，len==0 的情况是错误的

## 线程

Linux 中的线程就是一种轻量级进程，这和 rCore 中是不太一样的——线程创建是通过 `clone()`/`fork()` 系统调用完成的。

从 musl 的 pthread 源码中确实可以发现是进行了 `clone()` 系统调用的。

目前尚不清楚是否可以保持 rCore 的结构而提供同样语义的 `clone()` 函数实现。

## 信号机制

目前实现的信号机制是：

- 每个进程具有不同的动作，进程内的线程共享动作 (`SigHandlers`)
- 同一进程的每个线程可以有不同的信号掩码 (`SignalReceiver`)
- 信号的设置针对进程，但立刻被转发到合适的线程上（如果在该线程运行之前，其掩码又被设置为屏蔽该信号该如何？）

## Todo List

- [ ] Cow 虚拟页
- [ ] 页面置换
- [ ] 信号机制
- [ ] 标记 `unsafe`
