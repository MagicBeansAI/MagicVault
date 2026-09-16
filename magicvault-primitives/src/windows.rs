//! Windows user identity and protected DACLs. No shell or permission fallback.
use std::{
    ffi::c_void,
    fs::File,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::Threading::*,
};

pub fn wide(value: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
    let mut v: Vec<u16> = value.encode_wide().collect();
    if v.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    v.push(0);
    Ok(v)
}
pub fn user_sid(process: HANDLE) -> io::Result<Vec<u8>> {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut length = 0;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut length,
        );
        if length == 0 || length > 4096 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        // TOKEN_USER needs pointer alignment.
        let mut info = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            info.as_mut_ptr().cast(),
            length,
            &mut length,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let sid = (*(info.as_ptr() as *const TOKEN_USER)).User.Sid;
        if IsValidSid(sid) == 0 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(std::slice::from_raw_parts(sid.cast::<u8>(), GetLengthSid(sid) as usize).to_vec())
    }
}
pub fn current_sid_string() -> io::Result<String> {
    unsafe {
        let sid = user_sid(GetCurrentProcess())?;
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(sid.as_ptr() as PSID, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let _text = Local(text.cast());
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        String::from_utf16(std::slice::from_raw_parts(text, length))
            .map_err(|_| io::ErrorKind::InvalidData.into())
    }
}
pub fn same_process_user(pid: u32) -> bool {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let process = OwnedHandle::from_raw_handle(handle);
        match (
            user_sid(process.as_raw_handle()),
            user_sid(GetCurrentProcess()),
        ) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
pub struct PrivateSecurity {
    descriptor: Local,
}
impl PrivateSecurity {
    pub fn new() -> io::Result<Self> {
        unsafe {
            let sid = user_sid(GetCurrentProcess())?;
            let mut text = ptr::null_mut();
            if ConvertSidToStringSidW(sid.as_ptr() as PSID, &mut text) == 0 {
                return Err(io::Error::last_os_error());
            }
            let _text = Local(text.cast());
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let sid = String::from_utf16(std::slice::from_raw_parts(text, length))
                .map_err(|_| io::ErrorKind::InvalidData)?;
            // Protected, inheritable permissions for this user and SYSTEM only.
            let sddl = wide(std::ffi::OsStr::new(&format!(
                "O:{sid}D:P(A;OICI;FA;;;{sid})(A;OICI;FA;;;SY)"
            )))?;
            let mut descriptor = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                descriptor: Local(descriptor),
            })
        }
    }
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: 0,
        }
    }
}
pub fn create_dir(path: &Path) -> io::Result<()> {
    let name = wide(path.as_os_str())?;
    let security = PrivateSecurity::new()?;
    if unsafe { CreateDirectoryW(name.as_ptr(), &security.attributes()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn create_file(path: &Path) -> io::Result<File> {
    let name = wide(path.as_os_str())?;
    let security = PrivateSecurity::new()?;
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &security.attributes(),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_WRITE_THROUGH | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}
pub fn check_private(path: &Path) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    if std::fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    unsafe {
        let name = wide(path.as_os_str())?;
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        let error = GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        );
        if error != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let _descriptor = Local(descriptor);
        let user = user_sid(GetCurrentProcess())?;
        if owner.is_null() || dacl.is_null() || EqualSid(owner, user.as_ptr() as PSID) == 0 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let mut permitted = false;
        for index in 0..(*dacl).AceCount {
            let mut ace = ptr::null_mut();
            if GetAce(dacl, index as u32, &mut ace) == 0 {
                return Err(io::Error::last_os_error());
            }
            let header = &*(ace as *const ACE_HEADER);
            // Refuse unknown/object/callback ACEs, rather than interpreting a
            // permissive ACL as equivalent to the narrow private-file contract.
            if header.AceType != 0
            /* ACCESS_ALLOWED_ACE_TYPE */
            {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            let allowed = &*(ace as *const ACCESS_ALLOWED_ACE);
            let sid = (&allowed.SidStart as *const u32) as PSID;
            if IsValidSid(sid) == 0 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            if EqualSid(sid, user.as_ptr() as PSID) != 0 {
                permitted = true;
            } else if IsWellKnownSid(sid, WinLocalSystemSid) == 0 {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
        }
        if !permitted {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }
}
pub fn replace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide(source.as_os_str())?;
    let destination = wide(destination.as_os_str())?;
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_publication_preserves_acl_and_rejects_world_access() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("private");
        create_dir(&root).unwrap();
        check_private(&root).unwrap();
        let path = root.join("record");
        for value in [b"first".as_slice(), b"replacement"] {
            crate::durable_io::write_bytes_durably_with_mode_sync(&path, value, Some(0o600))
                .unwrap();
            check_private(&path).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), value);
        }
        assert!(create_file(&path).is_err());
        // Deliberately widen only this disposable fixture, then verify refusal.
        unsafe {
            let sddl = wide(std::ffi::OsStr::new("D:P(A;;FA;;;WD)")).unwrap();
            let mut descriptor = ptr::null_mut();
            assert_ne!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    ptr::null_mut()
                ),
                0
            );
            let _descriptor = Local(descriptor);
            let mut present = 0;
            let mut defaulted = 0;
            let mut dacl = ptr::null_mut();
            assert_ne!(
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted),
                0
            );
            let name = wide(path.as_os_str()).unwrap();
            assert_eq!(
                SetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    dacl,
                    ptr::null_mut()
                ),
                ERROR_SUCCESS
            );
        }
        assert!(check_private(&path).is_err());
    }

    #[test]
    fn identity_is_current_user_and_invalid_process_is_refused() {
        assert!(current_sid_string().unwrap().starts_with("S-1-"));
        assert!(same_process_user(std::process::id()));
        assert!(!same_process_user(0));
    }
}
