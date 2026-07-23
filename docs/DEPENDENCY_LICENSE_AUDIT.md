# Windows Dependency License Audit

**English** | [简体中文](DEPENDENCY_LICENSE_AUDIT_ZH.md)

Audit date: 2026-07-23

Target platform: `x86_64-pc-windows-msvc`

Dependency baseline: the repository's current `Cargo.lock`

PulseHub original source license: MIT

## Conclusion

After resolving the locked dependencies for the Windows target:

- All PulseHub workspace packages declare the `MIT` license.
- Regular third-party Rust dependencies declare MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, BSL-1.0, 0BSD, Unlicense, or multi-license combinations that include these permissive licenses.
- Slint 1.17.1 packages declare `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0`. PulseHub selects the Slint Royalty-free 2.0 path applicable to desktop applications and satisfies its attribution condition through the publicly visible official badge in the README.
- The Windows target resolution found no dependency for which GPL or LGPL is the only available choice without a permissive or Slint Royalty-free alternative.
- libratbag is not a Cargo dependency and is not distributed with PulseHub. The repository uses it as a protocol-compatibility research and test cross-check reference and retains its MIT notice in `THIRD_PARTY_NOTICES.md`.

## Audit method

Resolve the Windows build set from the locked dependencies:

```powershell
cargo metadata --format-version 1 --locked --filter-platform x86_64-pc-windows-msvc
```

Then review the SPDX `license` field for every package in the resolved set, with particular attention to empty fields, `GPL`, `LGPL`, custom `LicenseRef` entries, multi-license choices, target-specific dependencies, and build dependencies.

## Slint license selection

PulseHub is a desktop application running on general-purpose Windows computers. It selects the **Slint Royalty-free Desktop, Mobile, and Web Applications License 2.0**, rather than the GPLv3 path. This license requires either an in-application `AboutSlint` element or the official Slint attribution badge on a public webpage. PulseHub uses the public README badge.

Official terms: <https://slint.dev/terms-and-conditions>

## v0.1.2 release review

Review date: 2026-07-23

Compared with v0.1.1, the only changes in `Cargo.lock` are PulseHub workspace package version updates from `0.1.1` to `0.1.2`. The third-party dependency set, versions, Cargo features, target platform, and third-party installer contents are unchanged, so the original license audit conclusion remains valid.

## Re-audit conditions

A new audit is required whenever any of the following occurs:

1. `Cargo.lock`, Cargo features, or direct dependencies change.
2. A build target is added or the installer contents change.
3. Third-party source such as libratbag is referenced, rewritten, or ported.
4. The Slint version or selected license terms change.
5. The third-party license inventory generated before a binary release differs from this audit.

This file is an engineering compliance record and does not constitute legal advice.
