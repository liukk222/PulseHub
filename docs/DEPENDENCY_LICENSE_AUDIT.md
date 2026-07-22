# Windows 依赖许可证审计

审计日期：2026-07-23

目标平台：`x86_64-pc-windows-msvc`

依赖基准：仓库当前 `Cargo.lock`

PulseHub 自有源码许可证：MIT

## 结论

对锁定依赖执行 Windows 目标解析后：

- PulseHub 工作区包统一声明为 `MIT`；
- 常规第三方 Rust 依赖声明为 MIT、Apache-2.0、BSD、ISC、Zlib、Unicode-3.0、BSL-1.0、0BSD、Unlicense 或这些宽松许可证的多许可证组合；
- Slint 1.17.1 相关包声明为 `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0`。PulseHub 选择适用于桌面应用的 Slint Royalty-free 2.0 路径，并通过 README 中公开可见的官方徽章履行署名条件；
- Windows 目标解析结果未发现只能选择 GPL 或 LGPL、且没有宽松许可证或 Slint Royalty-free 替代项的依赖；
- libratbag 不是 Cargo 依赖，也未随 PulseHub 打包。仓库将其作为协议兼容性研究与测试交叉验证参考，并在 `THIRD_PARTY_NOTICES.md` 保留其 MIT 声明。

## 审计方法

使用锁定依赖解析 Windows 构建集合：

```powershell
cargo metadata --format-version 1 --locked --filter-platform x86_64-pc-windows-msvc
```

随后检查解析集合中每个包的 SPDX `license` 字段，重点复核空字段、`GPL`、`LGPL`、自定义 `LicenseRef`、多许可证选择、目标平台依赖与构建依赖。

## Slint 许可选择

PulseHub 是运行于通用 Windows 计算机的桌面应用，选择 **Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0**，而不是 GPLv3 路径。该许可证要求应用内展示 `AboutSlint`，或在公开网页展示官方 Slint attribution badge。PulseHub 采用公开 README 徽章。

官方条款：<https://slint.dev/terms-and-conditions>

## 重新审计条件

以下任一情况发生时必须重新审计：

1. `Cargo.lock`、Cargo feature 或直接依赖发生变化；
2. 增加构建目标或改变安装包内容；
3. 引用、改写或移植 libratbag 等第三方源码；
4. Slint 版本或所选择的许可条款发生变化；
5. 发布二进制前生成的第三方许可证清单与本审计不一致。

本文件是工程合规记录，不构成法律意见。
