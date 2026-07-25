use std::{
    ffi::{OsStr, c_void},
    fs::{self, File},
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
    ptr::{null, null_mut, slice_from_raw_parts},
};

use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, PipeMode, ServerOptions};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_PIPE_BUSY, ERROR_SUCCESS, GENERIC_ALL,
        GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
        },
        CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GetAce, GetAclInformation,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
        GetTokenInformation, INHERIT_ONLY_ACE, IsValidAcl, IsValidSid, OBJECT_INHERIT_ACE,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        PSID, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        CREATE_NEW, CreateFileW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        MoveFileExW, OPEN_ALWAYS, OPEN_EXISTING, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
    },
    System::{
        Pipes::GetNamedPipeServerProcessId,
        SystemServices::ACCESS_ALLOWED_ACE_TYPE,
        Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

use crate::{PipeInstanceKind, pipe_configuration};

const PIPE_BUFFER_SIZE: u32 = 64 * 1024;

/// Reports whether a named-pipe client open failed because every instance is busy.
#[must_use]
pub fn is_named_pipe_busy(error: &io::Error) -> bool {
    is_windows_error(error, ERROR_PIPE_BUSY)
}

/// Verifies that a connected pipe belongs to the expected current-user process.
///
/// # Errors
///
/// Returns an error when the server process differs from discovery metadata,
/// cannot be inspected, or runs under a different token user.
pub fn verify_named_pipe_server(
    client: &NamedPipeClient,
    expected_process_id: u32,
) -> io::Result<()> {
    if expected_process_id == 0 {
        return Err(invalid_data(
            "named pipe server process id must not be zero",
        ));
    }
    let mut actual_process_id = 0_u32;
    // SAFETY: the client owns a connected named-pipe handle and the output
    // pointer refers to initialized writable storage.
    if unsafe {
        GetNamedPipeServerProcessId(
            client.as_raw_handle().cast::<c_void>(),
            &raw mut actual_process_id,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if actual_process_id != expected_process_id {
        return Err(permission_denied(
            "named pipe server does not match discovery metadata",
        ));
    }

    let process = ProcessHandle::open(actual_process_id)?;
    let token = TokenHandle::for_process(process.as_raw())?;
    if token_user_sid_string(token.as_raw())? != current_user_sid_string()? {
        return Err(permission_denied(
            "named pipe server is not owned by the current user",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum OwnerOnlyObject {
    File,
    Directory,
}

/// Creates a byte-mode named pipe whose DACL grants access only to the current user.
///
/// # Errors
///
/// Returns an error when the instance limit is invalid, the current user SID
/// cannot be read, the security descriptor cannot be built, or pipe creation
/// fails.
pub fn create_owner_only_named_pipe(
    pipe_name: &OsStr,
    instance: PipeInstanceKind,
    max_instances: u8,
) -> io::Result<NamedPipeServer> {
    let configuration = pipe_configuration(instance, max_instances)?;
    let descriptor = SecurityDescriptor::for_current_user(OwnerOnlyObject::File)?;
    let mut attributes = descriptor.attributes()?;
    let mut options = ServerOptions::new();
    options
        .pipe_mode(PipeMode::Byte)
        .access_inbound(true)
        .access_outbound(true)
        .reject_remote_clients(true)
        .first_pipe_instance(configuration.first_instance)
        .max_instances(configuration.max_instances)
        .in_buffer_size(PIPE_BUFFER_SIZE)
        .out_buffer_size(PIPE_BUFFER_SIZE);

    // SAFETY: `attributes` and its owned security descriptor remain valid for
    // the entire synchronous CreateNamedPipeW call. The handle is non-inheritable.
    let result = unsafe {
        options.create_with_security_attributes_raw(
            pipe_name,
            std::ptr::from_mut(&mut attributes).cast::<c_void>(),
        )
    };
    drop(descriptor);
    result
}

/// Replaces a directory's owner and DACL with current-user-only security.
///
/// Child files and directories inherit the owner-only rule.
///
/// # Errors
///
/// Returns an error when the path is not a real directory or Windows cannot
/// apply and verify the current-user owner and protected DACL.
pub fn secure_owner_only_directory(path: &Path) -> io::Result<()> {
    let directory = open_path(
        path,
        READ_CONTROL | WRITE_DAC | WRITE_OWNER,
        OPEN_EXISTING,
        None,
    )?;
    verify_object_kind(&directory, OwnerOnlyObject::Directory)?;
    apply_owner_only_security(&directory, OwnerOnlyObject::Directory)?;
    verify_owner_only_security(&directory, OwnerOnlyObject::Directory)
}

/// Verifies that a directory is current-user-owned with a protected owner-only DACL.
///
/// # Errors
///
/// Returns an error when the path is a reparse point, is not a directory, or
/// does not have the required owner and DACL.
pub fn verify_owner_only_directory(path: &Path) -> io::Result<()> {
    let directory = open_path(path, READ_CONTROL, OPEN_EXISTING, None)?;
    verify_object_kind(&directory, OwnerOnlyObject::Directory)?;
    verify_owner_only_security(&directory, OwnerOnlyObject::Directory)
}

/// Creates a new current-user-only regular file and returns its open handle.
///
/// # Errors
///
/// Returns an error when the path already exists, file creation fails, or the
/// resulting owner and DACL cannot be verified.
pub fn create_new_owner_only_file(path: &Path) -> io::Result<File> {
    let descriptor = SecurityDescriptor::for_current_user(OwnerOnlyObject::File)?;
    let attributes = descriptor.attributes()?;
    let file = open_path(
        path,
        GENERIC_READ | GENERIC_WRITE | READ_CONTROL | WRITE_DAC,
        CREATE_NEW,
        Some(&attributes),
    )?;
    verify_object_kind(&file, OwnerOnlyObject::File)?;
    verify_owner_only_security(&file, OwnerOnlyObject::File)?;
    Ok(file)
}

/// Opens or creates a regular file, then applies current-user-only security.
///
/// # Errors
///
/// Returns an error when the path is a reparse point or the current-user owner
/// and protected DACL cannot be applied and verified.
pub fn open_or_create_owner_only_file(path: &Path) -> io::Result<File> {
    let descriptor = SecurityDescriptor::for_current_user(OwnerOnlyObject::File)?;
    let attributes = descriptor.attributes()?;
    let file = open_path(
        path,
        GENERIC_READ | GENERIC_WRITE | READ_CONTROL | WRITE_DAC | WRITE_OWNER,
        OPEN_ALWAYS,
        Some(&attributes),
    )?;
    verify_object_kind(&file, OwnerOnlyObject::File)?;
    apply_owner_only_security(&file, OwnerOnlyObject::File)?;
    verify_owner_only_security(&file, OwnerOnlyObject::File)?;
    Ok(file)
}

/// Opens a regular file only after verifying its owner and protected DACL.
///
/// The returned handle refers to the same object whose security was checked.
///
/// # Errors
///
/// Returns an error when the path is a reparse point, is not a regular file,
/// cannot be opened, or does not have the required owner and DACL.
pub fn open_owner_only_file(path: &Path) -> io::Result<File> {
    let file = open_path(path, GENERIC_READ | READ_CONTROL, OPEN_EXISTING, None)?;
    verify_object_kind(&file, OwnerOnlyObject::File)?;
    verify_owner_only_security(&file, OwnerOnlyObject::File)?;
    Ok(file)
}

/// Atomically replaces `destination` with an owner-only `source` file.
///
/// # Errors
///
/// Returns an error when either path contains an interior NUL, `source` is not
/// owner-only, Windows cannot complete the replacement, or the resulting
/// destination cannot be verified as owner-only.
pub fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    let source_file = open_owner_only_file(source)?;
    let encoded_source = null_terminated_path(source)?;
    let encoded_destination = null_terminated_path(destination)?;
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;

    // SAFETY: both paths are valid, NUL-terminated UTF-16 buffers that remain
    // alive for the duration of the call.
    if unsafe { MoveFileExW(encoded_source.as_ptr(), encoded_destination.as_ptr(), flags) } == 0 {
        return Err(io::Error::last_os_error());
    }
    drop(source_file);
    if let Err(error) = open_owner_only_file(destination) {
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}

fn open_path(
    path: &Path,
    desired_access: u32,
    creation_disposition: u32,
    attributes: Option<&SECURITY_ATTRIBUTES>,
) -> io::Result<File> {
    let encoded = null_terminated_path(path)?;
    let attributes = attributes.map_or(null(), std::ptr::from_ref);
    let share_mode = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let flags = FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS;
    // SAFETY: the path and optional SECURITY_ATTRIBUTES remain valid for the
    // synchronous call. A successful handle is transferred into `File` below.
    let handle = unsafe {
        CreateFileW(
            encoded.as_ptr(),
            desired_access,
            share_mode,
            attributes,
            creation_disposition,
            flags,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a new owned handle, which `File` closes once.
    Ok(unsafe { File::from_raw_handle(handle) })
}

fn verify_object_kind(file: &File, expected: OwnerOnlyObject) -> io::Result<()> {
    let metadata = file.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(permission_denied("owner-only IPC path is a reparse point"));
    }
    let matches = match expected {
        OwnerOnlyObject::File => metadata.is_file(),
        OwnerOnlyObject::Directory => metadata.is_dir(),
    };
    if !matches {
        return Err(permission_denied(
            "owner-only IPC path has an unexpected object type",
        ));
    }
    Ok(())
}

fn apply_owner_only_security(file: &File, object: OwnerOnlyObject) -> io::Result<()> {
    let descriptor = SecurityDescriptor::for_current_user(object)?;
    let owner = descriptor.owner()?;
    let dacl = descriptor.dacl()?;
    let security_information = OWNER_SECURITY_INFORMATION
        | DACL_SECURITY_INFORMATION
        | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: the file handle has owner/DACL write access; both security
    // pointers belong to `descriptor`, which remains alive for this call.
    let status = unsafe {
        SetSecurityInfo(
            file.as_raw_handle().cast::<c_void>(),
            SE_FILE_OBJECT,
            security_information,
            owner,
            null_mut(),
            dacl.cast_const(),
            null_mut(),
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(win32_error(status))
    }
}

fn verify_owner_only_security(file: &File, object: OwnerOnlyObject) -> io::Result<()> {
    let (descriptor, owner, dacl) = query_security(file)?;
    let current_user = current_user_sid_string()?;
    if !sid_matches(&current_user, owner)? {
        return Err(permission_denied(
            "owner-only IPC object is not owned by the current user",
        ));
    }

    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: `descriptor` is a valid security descriptor returned by Windows;
    // both output pointers refer to initialized writable values.
    if unsafe {
        GetSecurityDescriptorControl(descriptor.as_ptr(), &raw mut control, &raw mut revision)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if control & SE_DACL_PROTECTED == 0 {
        return Err(permission_denied(
            "owner-only IPC DACL is not protected from inheritance",
        ));
    }
    if dacl.is_null() {
        return Err(permission_denied("owner-only IPC object has a null DACL"));
    }
    // SAFETY: the DACL pointer belongs to the live queried security descriptor.
    if unsafe { IsValidAcl(dacl) } == 0 {
        return Err(invalid_data("owner-only IPC object has an invalid DACL"));
    }

    let mut information = ACL_SIZE_INFORMATION::default();
    // SAFETY: the DACL is valid and `information` is writable storage of the
    // exact size requested for AclSizeInformation.
    if unsafe {
        GetAclInformation(
            dacl,
            (&raw mut information).cast::<c_void>(),
            dword_size::<ACL_SIZE_INFORMATION>()?,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if information.AceCount == 0 {
        return Err(permission_denied("owner-only IPC DACL is empty"));
    }

    let mut grants_object = false;
    let mut inherits_to_files = matches!(object, OwnerOnlyObject::File);
    let mut inherits_to_directories = matches!(object, OwnerOnlyObject::File);
    // NTFS may split one inheritable generic ACE into effective and
    // inherit-only ACEs, so validate the complete ACL by semantics.
    for index in 0..information.AceCount {
        let mut raw_ace = null_mut();
        // SAFETY: the validated ACL reports `AceCount` entries, and `raw_ace`
        // is a writable output pointer.
        if unsafe { GetAce(dacl, index, &raw mut raw_ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if raw_ace.is_null() {
            return Err(invalid_data("owner-only IPC DACL returned a null ACE"));
        }
        // SAFETY: GetAce returned a pointer to an ACE_HEADER in the live ACL.
        let header = unsafe { &*raw_ace.cast::<ACE_HEADER>() };
        if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE
            || usize::from(header.AceSize) < size_of::<ACCESS_ALLOWED_ACE>()
        {
            return Err(permission_denied(
                "owner-only IPC DACL has an unexpected ACE type",
            ));
        }
        let flags = u32::from(header.AceFlags);
        let inherit_only = flags & INHERIT_ONLY_ACE != 0;
        let has_inheritance = flags & (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) != 0;
        if flags & !allowed_ace_flags(object) != 0 || inherit_only && !has_inheritance {
            return Err(permission_denied(
                "owner-only IPC DACL has unexpected inheritance flags",
            ));
        }
        // SAFETY: the ACE type and minimum size were checked above.
        let allowed = unsafe { &*raw_ace.cast::<ACCESS_ALLOWED_ACE>() };
        if allowed.Mask & GENERIC_ALL == 0 && allowed.Mask & FILE_ALL_ACCESS != FILE_ALL_ACCESS {
            return Err(permission_denied(
                "owner-only IPC ACE does not grant full control",
            ));
        }
        let ace_sid = std::ptr::addr_of!(allowed.SidStart)
            .cast_mut()
            .cast::<c_void>();
        if !sid_matches(&current_user, ace_sid)? {
            return Err(permission_denied(
                "owner-only IPC ACE does not target the current user",
            ));
        }
        grants_object |= !inherit_only;
        inherits_to_files |= flags & OBJECT_INHERIT_ACE != 0;
        inherits_to_directories |= flags & CONTAINER_INHERIT_ACE != 0;
    }
    if !grants_object {
        return Err(permission_denied(
            "owner-only IPC DACL does not grant access to the object",
        ));
    }
    if !inherits_to_files || !inherits_to_directories {
        return Err(permission_denied(
            "owner-only IPC directory DACL does not cover child objects",
        ));
    }
    Ok(())
}

fn query_security(file: &File) -> io::Result<(SecurityDescriptor, PSID, *mut ACL)> {
    let mut owner = null_mut();
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    let security_information = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    // SAFETY: the file handle is valid and all requested output pointers refer
    // to initialized writable storage. Windows allocates `descriptor`.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle().cast::<c_void>(),
            SE_FILE_OBJECT,
            security_information,
            &raw mut owner,
            null_mut(),
            &raw mut dacl,
            null_mut(),
            &raw mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        if !descriptor.is_null() {
            // SAFETY: a non-null descriptor returned by GetSecurityInfo uses
            // LocalAlloc even when the API reports failure.
            let _ = unsafe { LocalFree(descriptor) };
        }
        return Err(win32_error(status));
    }
    let descriptor = SecurityDescriptor::from_raw(descriptor)?;
    Ok((descriptor, owner, dacl))
}

fn sid_matches(expected: &[u16], candidate: PSID) -> io::Result<bool> {
    if candidate.is_null() {
        return Ok(false);
    }
    // SAFETY: candidate comes from a validated Windows security descriptor or
    // ACE and remains alive for this call.
    if unsafe { IsValidSid(candidate) } == 0 {
        return Ok(false);
    }
    Ok(SidString::from_sid(candidate)?.to_vec()? == expected)
}

const fn allowed_ace_flags(object: OwnerOnlyObject) -> u32 {
    match object {
        OwnerOnlyObject::File => 0,
        OwnerOnlyObject::Directory => OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE | INHERIT_ONLY_ACE,
    }
}

fn dword_size<T>() -> io::Result<u32> {
    u32::try_from(size_of::<T>())
        .map_err(|_| io::Error::other("Windows structure size does not fit in a DWORD"))
}

fn win32_error(code: u32) -> io::Error {
    match i32::try_from(code) {
        Ok(code) => io::Error::from_raw_os_error(code),
        Err(_) => io::Error::other(format!("Windows error code {code}")),
    }
}

fn permission_denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn null_terminated_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut encoded = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if encoded.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows path contains an interior NUL",
        ));
    }
    encoded.push(0);
    Ok(encoded)
}

fn current_user_sid_string() -> io::Result<Vec<u16>> {
    let token = TokenHandle::current_process()?;
    token_user_sid_string(token.as_raw())
}

fn token_user_sid_string(token: HANDLE) -> io::Result<Vec<u16>> {
    let mut required_size = 0_u32;

    // SAFETY: the null buffer and zero length form the documented size query;
    // `required_size` points to initialized writable storage.
    let size_query =
        unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &raw mut required_size) };
    if size_query != 0 {
        return Err(invalid_data(
            "token user size query unexpectedly succeeded without a buffer",
        ));
    }
    let size_error = io::Error::last_os_error();
    if !is_windows_error(&size_error, ERROR_INSUFFICIENT_BUFFER) {
        return Err(size_error);
    }
    if usize::try_from(required_size).map_err(|_| invalid_data("token user size overflow"))?
        < size_of::<TOKEN_USER>()
    {
        return Err(invalid_data("token user information is truncated"));
    }

    let byte_length =
        usize::try_from(required_size).map_err(|_| invalid_data("token user size overflow"))?;
    let word_count = byte_length.div_ceil(size_of::<usize>());
    let mut storage = vec![0_usize; word_count];
    let mut returned_size = 0_u32;
    // SAFETY: `storage` is pointer-aligned and contains at least
    // `required_size` writable bytes. The token handle is valid.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            storage.as_mut_ptr().cast::<c_void>(),
            required_size,
            &raw mut returned_size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if returned_size > required_size {
        return Err(invalid_data("token user information exceeded its buffer"));
    }

    // SAFETY: the successful query initialized a TOKEN_USER at the start of
    // the suitably aligned buffer, whose minimum size was checked above.
    let token_user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let sid = token_user.User.Sid;
    if sid.is_null() {
        return Err(invalid_data("current user token contains a null SID"));
    }
    // SAFETY: the SID pointer belongs to the initialized TOKEN_USER buffer and
    // remains valid while `storage` is alive.
    if unsafe { IsValidSid(sid) } == 0 {
        return Err(invalid_data("current user token contains an invalid SID"));
    }
    SidString::from_sid(sid)?.to_vec()
}

fn is_windows_error(error: &io::Error, expected: u32) -> bool {
    error
        .raw_os_error()
        .and_then(|code| u32::try_from(code).ok())
        == Some(expected)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

struct TokenHandle(HANDLE);

impl TokenHandle {
    fn current_process() -> io::Result<Self> {
        // SAFETY: GetCurrentProcess returns a borrowed pseudo-handle that stays
        // valid for this process and must not be closed.
        Self::for_process(unsafe { GetCurrentProcess() })
    }

    fn for_process(process: HANDLE) -> io::Result<Self> {
        let mut token = null_mut();
        // SAFETY: `process` is either the current-process pseudo-handle or a
        // live owned process handle. A successful token is owned by this guard.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if token.is_null() {
            return Err(invalid_data("OpenProcessToken returned a null handle"));
        }
        Ok(Self(token))
    }

    const fn as_raw(&self) -> HANDLE {
        self.0
    }
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32) -> io::Result<Self> {
        // SAFETY: the access mask and non-inheriting flag are valid, and Windows
        // returns either an owned process handle or null.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if process.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(process))
        }
    }

    const fn as_raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by OpenProcess and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

impl Drop for TokenHandle {
    fn drop(&mut self) {
        // SAFETY: the handle was returned by OpenProcessToken and is closed once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

struct SidString(*mut u16);

impl SidString {
    fn from_sid(sid: PSID) -> io::Result<Self> {
        let mut value = null_mut();
        // SAFETY: `sid` was validated and remains alive for this call. Windows
        // writes an allocated, NUL-terminated string pointer to `value`.
        if unsafe { ConvertSidToStringSidW(sid, &raw mut value) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if value.is_null() {
            return Err(invalid_data(
                "ConvertSidToStringSidW returned a null string",
            ));
        }
        Ok(Self(value))
    }

    fn to_vec(&self) -> io::Result<Vec<u16>> {
        let mut length = 0_usize;
        loop {
            // SAFETY: ConvertSidToStringSidW guarantees a NUL-terminated string.
            if unsafe { *self.0.add(length) } == 0 {
                break;
            }
            length = length
                .checked_add(1)
                .ok_or_else(|| invalid_data("current user SID string length overflow"))?;
        }
        // SAFETY: the scan above found the terminator, so these `length` code
        // units are initialized and contained in the allocated SID string.
        let slice = unsafe { &*slice_from_raw_parts(self.0, length) };
        Ok(slice.to_vec())
    }
}

impl Drop for SidString {
    fn drop(&mut self) {
        // SAFETY: this pointer was allocated by ConvertSidToStringSidW and is
        // released exactly once with LocalFree.
        let _ = unsafe { LocalFree(self.0.cast::<c_void>()) };
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn for_current_user(object: OwnerOnlyObject) -> io::Result<Self> {
        let sid = current_user_sid_string()?;
        let mut sddl = "O:".encode_utf16().collect::<Vec<_>>();
        sddl.extend_from_slice(&sid);
        sddl.extend("D:P(A;".encode_utf16());
        if matches!(object, OwnerOnlyObject::Directory) {
            sddl.extend("OICI".encode_utf16());
        }
        sddl.extend(";GA;;;".encode_utf16());
        sddl.extend_from_slice(&sid);
        sddl.extend(")".encode_utf16());
        sddl.push(0);

        let mut descriptor = null_mut();
        // SAFETY: `sddl` is a valid, NUL-terminated UTF-16 string and the
        // output pointer refers to initialized writable storage.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if descriptor.is_null() {
            return Err(invalid_data(
                "security descriptor conversion returned a null pointer",
            ));
        }
        Ok(Self(descriptor))
    }

    fn from_raw(descriptor: PSECURITY_DESCRIPTOR) -> io::Result<Self> {
        if descriptor.is_null() {
            Err(invalid_data("Windows returned a null security descriptor"))
        } else {
            Ok(Self(descriptor))
        }
    }

    fn attributes(&self) -> io::Result<SECURITY_ATTRIBUTES> {
        Ok(SECURITY_ATTRIBUTES {
            nLength: dword_size::<SECURITY_ATTRIBUTES>()?,
            lpSecurityDescriptor: self.as_ptr(),
            bInheritHandle: 0,
        })
    }

    fn dacl(&self) -> io::Result<*mut ACL> {
        let mut present = 0;
        let mut dacl = null_mut();
        let mut defaulted = 0;
        // SAFETY: this descriptor is valid and all output pointers refer to
        // initialized writable storage.
        if unsafe {
            GetSecurityDescriptorDacl(
                self.as_ptr(),
                &raw mut present,
                &raw mut dacl,
                &raw mut defaulted,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if present == 0 || dacl.is_null() {
            return Err(invalid_data(
                "owner-only security descriptor does not contain a DACL",
            ));
        }
        Ok(dacl)
    }

    fn owner(&self) -> io::Result<PSID> {
        let mut owner = null_mut();
        let mut defaulted = 0;
        // SAFETY: this descriptor is valid and both output pointers refer to
        // initialized writable storage.
        if unsafe { GetSecurityDescriptorOwner(self.as_ptr(), &raw mut owner, &raw mut defaulted) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        if owner.is_null() {
            return Err(invalid_data(
                "owner-only security descriptor does not contain an owner",
            ));
        }
        Ok(owner)
    }

    const fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        self.0
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: this pointer was allocated by the SDDL conversion API and is
        // released exactly once with LocalFree.
        let _ = unsafe { LocalFree(self.0) };
    }
}
