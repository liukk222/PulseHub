//! Windows 登录会话标识的最小安全封装。

use std::io;

#[cfg(windows)]
pub fn current_logon_sid() -> io::Result<String> {
    use std::ffi::c_void;
    use std::ptr;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_GROUPS, TOKEN_QUERY, TokenLogonSid,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    struct Token(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: `OpenProcessToken` 成功后返回由本对象唯一持有的有效句柄。
            unsafe { CloseHandle(self.0) };
        }
    }

    let mut raw_token = ptr::null_mut();
    // SAFETY: 输出指针有效，成功后的句柄立即交给 RAII 包装。
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Token(raw_token);

    let mut required = 0_u32;
    // SAFETY: 首次调用按 API 约定以空缓冲区查询所需长度。
    let first =
        unsafe { GetTokenInformation(token.0, TokenLogonSid, ptr::null_mut(), 0, &mut required) };
    if first != 0
        || required == 0
        || io::Error::last_os_error().raw_os_error()
            != Some(i32::try_from(ERROR_INSUFFICIENT_BUFFER).unwrap())
    {
        return Err(io::Error::last_os_error());
    }

    let word = std::mem::size_of::<usize>();
    let words = usize::try_from(required)
        .unwrap_or(usize::MAX)
        .div_ceil(word);
    let mut buffer = vec![0_usize; words];
    // SAFETY: 缓冲区按 `usize` 对齐且至少为系统报告的字节长度。
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenLogonSid,
            buffer.as_mut_ptr().cast::<c_void>(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `TokenLogonSid` 成功时缓冲区包含 `TOKEN_GROUPS`，且至少一个 SID。
    let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
    if groups.GroupCount != 1 || groups.Groups[0].Sid.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TokenLogonSid 未返回唯一 SID",
        ));
    }

    let mut sid_text = ptr::null_mut();
    // SAFETY: SID 指针来自系统填充的有效 `TOKEN_GROUPS`，输出由 `LocalFree` 释放。
    if unsafe { ConvertSidToStringSidW(groups.Groups[0].Sid, &mut sid_text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    struct LocalString(*mut u16);
    impl Drop for LocalString {
        fn drop(&mut self) {
            // SAFETY: 指针由 `ConvertSidToStringSidW` 使用 LocalAlloc 分配。
            unsafe { LocalFree(self.0.cast()) };
        }
    }
    let sid_text = LocalString(sid_text);
    let mut length = 0_usize;
    // SAFETY: Windows 返回以 NUL 结尾的 UTF-16 字符串。
    while unsafe { *sid_text.0.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: 上面的扫描确定了初始化且不含终止 NUL 的范围。
    let units = unsafe { std::slice::from_raw_parts(sid_text.0, length) };
    String::from_utf16(units).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(not(windows))]
pub fn current_logon_sid() -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "登录 SID 查询仅支持 Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn logon_sid_has_expected_well_known_prefix() {
        let sid = current_logon_sid().unwrap();
        assert!(sid.starts_with("S-1-5-5-"), "unexpected logon SID: {sid}");
        assert!(
            sid[2..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'-')
        );
    }
}
