//! PulseHub 的 Slint 生成代码边界。
//!
//! Slint 生成代码内部包含框架维护的 unsafe 实现，因此仅此 crate 放宽 unsafe lint；
//! `pulsehub-config` 及所有领域、IPC 和设备 crate 仍保持 `forbid(unsafe_code)`。

#![allow(unsafe_code)]

slint::include_modules!();
