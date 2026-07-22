# 第三方声明

**简体中文** | [English](THIRD_PARTY_NOTICES.md)

PulseHub 自有源码采用 MIT 许可证。构建与分发 PulseHub 时还须遵守所使用的第三方组件各自的许可证；依赖的精确版本以 `Cargo.lock` 为准。

## Slint

PulseHub 使用 Slint 1.17.1 构建桌面界面，并选择 **Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0** 作为 Slint 的使用路径。PulseHub 在公开仓库 README 展示官方 “Made with Slint” 徽章，以履行公开网页署名条件。

- 项目：<https://slint.dev/>
- 许可选项：<https://slint.dev/terms-and-conditions>

Slint 及其所含第三方组件仍归各自权利人所有；不得移除或更改其源代码中的许可证、版权、免责声明或责任限制声明。

## libratbag

PulseHub 的 Logitech HID++ 兼容性研究参考了 libratbag 的公开设备资料和实现，用于协议理解、兼容性研究与测试交叉验证。libratbag 不是 Logitech 对 PulseHub 的协议承诺。

若 PulseHub 中包含或后续引入 libratbag 软件的副本或实质性部分，下列原始 MIT 声明必须随相应内容一并保留：

> Copyright © 2015-2017 Red Hat, Inc.
>
> Copyright © 2015 David Herrmann <dh.herrmann@gmail.com>
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice (including the next
> paragraph) shall be included in all copies or substantial portions of the
> Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
> THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
> FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
> IN THE SOFTWARE.

- 项目：<https://github.com/libratbag/libratbag>
- 原始许可：<https://github.com/libratbag/libratbag/blob/master/COPYING>

## 其他 Rust 依赖

PulseHub 还使用 `Cargo.lock` 中列出的 Rust 依赖。Windows 目标的审计结果记录在 [`docs/DEPENDENCY_LICENSE_AUDIT.md`](docs/DEPENDENCY_LICENSE_AUDIT.md)。发布二进制文件前，应根据锁定版本生成并复核完整的依赖许可证清单，并随发行包提供相应版权与许可证文本。本文件不改变任何第三方组件的许可证。

## Logitech 名称和设备

PulseHub 是独立开源项目，与 Logitech 没有隶属、授权或背书关系。Logitech、Logitech G、G102 LIGHTSYNC 及相关产品名称和标识归其各自权利人所有。
