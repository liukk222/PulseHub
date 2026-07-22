# Third-party notices

[简体中文](THIRD_PARTY_NOTICES_ZH.md) | **English**

PulseHub's original source code is licensed under the MIT License. Building and distributing PulseHub also requires compliance with the licenses of the third-party components used by the project. Exact dependency versions are recorded in `Cargo.lock`.

## Slint

PulseHub uses Slint 1.17.1 for its desktop interface under the **Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0**. PulseHub displays the official “Made with Slint” badge in the public repository README to satisfy the public-webpage attribution condition.

- Project: <https://slint.dev/>
- License options: <https://slint.dev/terms-and-conditions>

Slint and its bundled third-party components remain the property of their respective rights holders. License notices, copyright notices, warranty disclaimers, and limitations of liability contained in Slint source code must not be removed or altered.

## libratbag

PulseHub's Logitech HID++ compatibility research referred to public device information and implementations from libratbag for protocol understanding, compatibility research, and testing cross-validation. libratbag does not constitute a Logitech stability commitment to PulseHub.

If PulseHub includes, or later introduces, a copy or substantial portion of libratbag software, the following original MIT notice must be retained with that material:

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

- Project: <https://github.com/libratbag/libratbag>
- Original license: <https://github.com/libratbag/libratbag/blob/master/COPYING>

## Other Rust dependencies

PulseHub also uses the Rust dependencies recorded in `Cargo.lock`. The Windows-target audit is available in [`docs/DEPENDENCY_LICENSE_AUDIT.md`](docs/DEPENDENCY_LICENSE_AUDIT.md). Before distributing binaries, regenerate and review a complete dependency-license list for the locked versions and include all required copyright and license texts with the distribution. This document does not modify the license of any third-party component.

## Logitech names and devices

PulseHub is an independent open-source project and is not affiliated with, authorized by, or endorsed by Logitech. Logitech, Logitech G, G102 LIGHTSYNC, and related product names and marks belong to their respective rights holders.
