# Granite

## 规范

加载器加载 ELF 可执行文件时，`argc`、`argv`、参数、环境变量、`auxv`、如何排布是有具体要求的。

可以参考：

- <http://articles.manugarg.com/aboutelfauxiliaryvectors.html>
- <https://gitlab.eduxiji.net/scPointer/maturin/-/blob/master/kernel/src/loaders/init_info.rs#L27>
- <http://www.lenky.info/archives/2013/02/2203>

res/abi386-4.pdf 中的 Figure 3-31 也有比较粗略的描述。

`auxv` 即辅助向量，可以参考 <https://blog.csdn.net/choumin/article/details/111385498>
