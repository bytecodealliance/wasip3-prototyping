use crate::fs::{DirPerms, FilePerms, OpenMode};
use crate::p3::bindings::clocks::wall_clock;
use crate::p3::bindings::filesystem;
use crate::p3::bindings::filesystem::types::{
    Advice, DescriptorFlags, DescriptorStat, DescriptorType, ErrorCode, MetadataHashValue,
    NewTimestamp, OpenFlags, PathFlags,
};
use crate::p3::{ResourceView, TaskTable};
use crate::runtime::{AbortOnDropJoinHandle, spawn_blocking};
use cap_fs_ext::{FileTypeExt as _, MetadataExt as _};
use cap_std::ambient_authority;
use std::collections::hash_map;
use std::path::Path;
use std::sync::Arc;
use tracing::debug;
use wasmtime::component::{HasData, Linker, ResourceTable};

mod host;

#[repr(transparent)]
pub struct WasiFilesystemImpl<T>(pub T);

impl<T: WasiFilesystemView> WasiFilesystemView for &mut T {
    fn filesystem(&self) -> &WasiFilesystemCtx {
        (**self).filesystem()
    }
}

impl<T: WasiFilesystemView> WasiFilesystemView for WasiFilesystemImpl<T> {
    fn filesystem(&self) -> &WasiFilesystemCtx {
        self.0.filesystem()
    }
}

impl<T: ResourceView> ResourceView for WasiFilesystemImpl<T> {
    fn table(&mut self) -> &mut ResourceTable {
        self.0.table()
    }
}

pub trait WasiFilesystemView: ResourceView + Send {
    fn filesystem(&self) -> &WasiFilesystemCtx;
}

#[derive(Clone, Default)]
pub struct WasiFilesystemCtx {
    pub preopens: Vec<(Dir, String)>,
    pub allow_blocking_current_thread: bool,
}

impl WasiFilesystemCtx {
    /// Configures a "preopened directory" to be available to WebAssembly.
    ///
    /// By default WebAssembly does not have access to the filesystem because
    /// there are no preopened directories. All filesystem operations, such as
    /// opening a file, are done through a preexisting handle. This means that
    /// to provide WebAssembly access to a directory it must be configured
    /// through this API.
    ///
    /// WASI will also prevent access outside of files provided here. For
    /// example `..` can't be used to traverse up from the `host_path` provided here
    /// to the containing directory.
    ///
    /// * `host_path` - a path to a directory on the host to open and make
    ///   accessible to WebAssembly. Note that the name of this directory in the
    ///   guest is configured with `guest_path` below.
    /// * `guest_path` - the name of the preopened directory from WebAssembly's
    ///   perspective. Note that this does not need to match the host's name for
    ///   the directory.
    /// * `dir_perms` - this is the permissions that wasm will have to operate on
    ///   `guest_path`. This can be used, for example, to provide readonly access to a
    ///   directory.
    /// * `file_perms` - similar to `dir_perms` but corresponds to the maximum set
    ///   of permissions that can be used for any file in this directory.
    ///
    /// # Errors
    ///
    /// This method will return an error if `host_path` cannot be opened.
    ///
    /// # Examples
    ///
    /// ```
    /// use wasmtime_wasi::{p2::WasiCtxBuilder, DirPerms, FilePerms};
    ///
    /// # fn main() {}
    /// # fn foo() -> wasmtime::Result<()> {
    /// let mut wasi = WasiCtxBuilder::new();
    ///
    /// // Make `./host-directory` available in the guest as `.`
    /// wasi.preopened_dir("./host-directory", ".", DirPerms::all(), FilePerms::all());
    ///
    /// // Make `./readonly` available in the guest as `./ro`
    /// wasi.preopened_dir("./readonly", "./ro", DirPerms::READ, FilePerms::READ);
    /// # Ok(())
    /// # }
    /// ```
    pub fn preopened_dir(
        &mut self,
        host_path: impl AsRef<Path>,
        guest_path: impl Into<String>,
        dir_perms: DirPerms,
        file_perms: FilePerms,
    ) -> wasmtime::Result<&mut Self> {
        let dir = cap_std::fs::Dir::open_ambient_dir(host_path.as_ref(), ambient_authority())?;
        let mut open_mode = OpenMode::empty();
        if dir_perms.contains(DirPerms::READ) {
            open_mode |= OpenMode::READ;
        }
        if dir_perms.contains(DirPerms::MUTATE) {
            open_mode |= OpenMode::WRITE;
        }
        self.preopens.push((
            Dir::new(
                dir,
                dir_perms,
                file_perms,
                open_mode,
                self.allow_blocking_current_thread,
            ),
            guest_path.into(),
        ));
        Ok(self)
    }
}

/// Add all WASI interfaces from this module into the `linker` provided.
///
/// This function will add the `async` variant of all interfaces into the
/// [`Linker`] provided. By `async` this means that this function is only
/// compatible with [`Config::async_support(true)`][async]. For embeddings with
/// async support disabled see [`add_to_linker_sync`] instead.
///
/// This function will add all interfaces implemented by this crate to the
/// [`Linker`], which corresponds to the `wasi:filesystem/imports` world supported by
/// this crate.
///
/// [async]: wasmtime::Config::async_support
///
/// # Example
///
/// ```
/// use wasmtime::{Engine, Result, Store, Config};
/// use wasmtime::component::{ResourceTable, Linker};
/// use wasmtime_wasi::p3::filesystem::{WasiFilesystemView, WasiFilesystemCtx};
/// use wasmtime_wasi::p3::ResourceView;
///
/// fn main() -> Result<()> {
///     let mut config = Config::new();
///     config.async_support(true);
///     let engine = Engine::new(&config)?;
///
///     let mut linker = Linker::<MyState>::new(&engine);
///     wasmtime_wasi::p3::filesystem::add_to_linker(&mut linker)?;
///     // ... add any further functionality to `linker` if desired ...
///
///     let mut store = Store::new(
///         &engine,
///         MyState {
///             filesystem: WasiFilesystemCtx::default(),
///             table: ResourceTable::default(),
///         },
///     );
///
///     // ... use `linker` to instantiate within `store` ...
///
///     Ok(())
/// }
///
/// struct MyState {
///     filesystem: WasiFilesystemCtx,
///     table: ResourceTable,
/// }
///
/// impl ResourceView for MyState {
///     fn table(&mut self) -> &mut ResourceTable { &mut self.table }
/// }
///
/// impl WasiFilesystemView for MyState {
///     fn filesystem(&self) -> &WasiFilesystemCtx { &self.filesystem }
/// }
/// ```
pub fn add_to_linker<T: WasiFilesystemView + 'static>(
    linker: &mut Linker<T>,
) -> wasmtime::Result<()> {
    filesystem::types::add_to_linker::<_, WasiFilesystem<T>>(linker, |x| WasiFilesystemImpl(x))?;
    filesystem::preopens::add_to_linker::<_, WasiFilesystem<T>>(linker, |x| WasiFilesystemImpl(x))?;
    Ok(())
}

struct WasiFilesystem<T>(T);

impl<T: 'static> HasData for WasiFilesystem<T> {
    type Data<'a> = WasiFilesystemImpl<&'a mut T>;
}

fn datetime_from(t: std::time::SystemTime) -> wall_clock::Datetime {
    // FIXME make this infallible or handle errors properly
    wall_clock::Datetime::try_from(cap_std::time::SystemTime::from_std(t)).unwrap()
}

fn systemtime_from(t: wall_clock::Datetime) -> Result<std::time::SystemTime, ErrorCode> {
    std::time::SystemTime::UNIX_EPOCH
        .checked_add(core::time::Duration::new(t.seconds, t.nanoseconds))
        .ok_or(ErrorCode::Overflow)
}

fn systemtimespec_from(t: NewTimestamp) -> Result<Option<fs_set_times::SystemTimeSpec>, ErrorCode> {
    use fs_set_times::SystemTimeSpec;
    match t {
        NewTimestamp::NoChange => Ok(None),
        NewTimestamp::Now => Ok(Some(SystemTimeSpec::SymbolicNow)),
        NewTimestamp::Timestamp(st) => {
            let st = systemtime_from(st)?;
            Ok(Some(SystemTimeSpec::Absolute(st)))
        }
    }
}

impl From<cap_std::fs::FileType> for DescriptorType {
    fn from(ft: cap_std::fs::FileType) -> Self {
        if ft.is_dir() {
            DescriptorType::Directory
        } else if ft.is_symlink() {
            DescriptorType::SymbolicLink
        } else if ft.is_block_device() {
            DescriptorType::BlockDevice
        } else if ft.is_char_device() {
            DescriptorType::CharacterDevice
        } else if ft.is_file() {
            DescriptorType::RegularFile
        } else {
            DescriptorType::Unknown
        }
    }
}

impl From<cap_std::fs::Metadata> for DescriptorStat {
    fn from(meta: cap_std::fs::Metadata) -> Self {
        Self {
            type_: meta.file_type().into(),
            link_count: meta.nlink(),
            size: meta.len(),
            data_access_timestamp: meta.accessed().map(|t| datetime_from(t.into_std())).ok(),
            data_modification_timestamp: meta.modified().map(|t| datetime_from(t.into_std())).ok(),
            status_change_timestamp: meta.created().map(|t| datetime_from(t.into_std())).ok(),
        }
    }
}

impl From<&cap_std::fs::Metadata> for MetadataHashValue {
    fn from(meta: &cap_std::fs::Metadata) -> Self {
        use cap_fs_ext::MetadataExt;
        // Without incurring any deps, std provides us with a 64 bit hash
        // function:
        use std::hash::Hasher;
        // Note that this means that the metadata hash (which becomes a preview1 ino) may
        // change when a different rustc release is used to build this host implementation:
        let mut hasher = hash_map::DefaultHasher::new();
        hasher.write_u64(meta.dev());
        hasher.write_u64(meta.ino());
        let lower = hasher.finish();
        // MetadataHashValue has a pair of 64-bit members for representing a
        // single 128-bit number. However, we only have 64 bits of entropy. To
        // synthesize the upper 64 bits, lets xor the lower half with an arbitrary
        // constant, in this case the 64 bit integer corresponding to the IEEE
        // double representation of (a number as close as possible to) pi.
        // This seems better than just repeating the same bits in the upper and
        // lower parts outright, which could make folks wonder if the struct was
        // mangled in the ABI, or worse yet, lead to consumers of this interface
        // expecting them to be equal.
        let upper = lower ^ 4614256656552045848u64;
        Self { lower, upper }
    }
}

#[cfg(unix)]
fn from_raw_os_error(err: Option<i32>) -> Option<ErrorCode> {
    use rustix::io::Errno as RustixErrno;
    if err.is_none() {
        return None;
    }
    Some(match RustixErrno::from_raw_os_error(err.unwrap()) {
        RustixErrno::PIPE => ErrorCode::Pipe,
        RustixErrno::PERM => ErrorCode::NotPermitted,
        RustixErrno::NOENT => ErrorCode::NoEntry,
        RustixErrno::NOMEM => ErrorCode::InsufficientMemory,
        RustixErrno::IO => ErrorCode::Io,
        RustixErrno::BADF => ErrorCode::BadDescriptor,
        RustixErrno::BUSY => ErrorCode::Busy,
        RustixErrno::ACCESS => ErrorCode::Access,
        RustixErrno::NOTDIR => ErrorCode::NotDirectory,
        RustixErrno::ISDIR => ErrorCode::IsDirectory,
        RustixErrno::INVAL => ErrorCode::Invalid,
        RustixErrno::EXIST => ErrorCode::Exist,
        RustixErrno::FBIG => ErrorCode::FileTooLarge,
        RustixErrno::NOSPC => ErrorCode::InsufficientSpace,
        RustixErrno::SPIPE => ErrorCode::InvalidSeek,
        RustixErrno::MLINK => ErrorCode::TooManyLinks,
        RustixErrno::NAMETOOLONG => ErrorCode::NameTooLong,
        RustixErrno::NOTEMPTY => ErrorCode::NotEmpty,
        RustixErrno::LOOP => ErrorCode::Loop,
        RustixErrno::OVERFLOW => ErrorCode::Overflow,
        RustixErrno::ILSEQ => ErrorCode::IllegalByteSequence,
        RustixErrno::NOTSUP => ErrorCode::Unsupported,
        RustixErrno::ALREADY => ErrorCode::Already,
        RustixErrno::INPROGRESS => ErrorCode::InProgress,
        RustixErrno::INTR => ErrorCode::Interrupted,

        // On some platforms, these have the same value as other errno values.
        #[allow(unreachable_patterns, reason = "see comment")]
        RustixErrno::OPNOTSUPP => ErrorCode::Unsupported,

        _ => return None,
    })
}

#[cfg(windows)]
fn from_raw_os_error(raw_os_error: Option<i32>) -> Option<ErrorCode> {
    use windows_sys::Win32::Foundation;
    Some(match raw_os_error.map(|code| code as u32) {
        Some(Foundation::ERROR_FILE_NOT_FOUND) => ErrorCode::NoEntry,
        Some(Foundation::ERROR_PATH_NOT_FOUND) => ErrorCode::NoEntry,
        Some(Foundation::ERROR_ACCESS_DENIED) => ErrorCode::Access,
        Some(Foundation::ERROR_SHARING_VIOLATION) => ErrorCode::Access,
        Some(Foundation::ERROR_PRIVILEGE_NOT_HELD) => ErrorCode::NotPermitted,
        Some(Foundation::ERROR_INVALID_HANDLE) => ErrorCode::BadDescriptor,
        Some(Foundation::ERROR_INVALID_NAME) => ErrorCode::NoEntry,
        Some(Foundation::ERROR_NOT_ENOUGH_MEMORY) => ErrorCode::InsufficientMemory,
        Some(Foundation::ERROR_OUTOFMEMORY) => ErrorCode::InsufficientMemory,
        Some(Foundation::ERROR_DIR_NOT_EMPTY) => ErrorCode::NotEmpty,
        Some(Foundation::ERROR_NOT_READY) => ErrorCode::Busy,
        Some(Foundation::ERROR_BUSY) => ErrorCode::Busy,
        Some(Foundation::ERROR_NOT_SUPPORTED) => ErrorCode::Unsupported,
        Some(Foundation::ERROR_FILE_EXISTS) => ErrorCode::Exist,
        Some(Foundation::ERROR_BROKEN_PIPE) => ErrorCode::Pipe,
        Some(Foundation::ERROR_BUFFER_OVERFLOW) => ErrorCode::NameTooLong,
        Some(Foundation::ERROR_NOT_A_REPARSE_POINT) => ErrorCode::Invalid,
        Some(Foundation::ERROR_NEGATIVE_SEEK) => ErrorCode::Invalid,
        Some(Foundation::ERROR_DIRECTORY) => ErrorCode::NotDirectory,
        Some(Foundation::ERROR_ALREADY_EXISTS) => ErrorCode::Exist,
        Some(Foundation::ERROR_STOPPED_ON_SYMLINK) => ErrorCode::Loop,
        Some(Foundation::ERROR_DIRECTORY_NOT_SUPPORTED) => ErrorCode::IsDirectory,
        _ => return None,
    })
}

impl<'a> From<&'a std::io::Error> for ErrorCode {
    fn from(err: &'a std::io::Error) -> ErrorCode {
        match from_raw_os_error(err.raw_os_error()) {
            Some(errno) => errno,
            None => {
                debug!("unknown raw os error: {err}");
                match err.kind() {
                    std::io::ErrorKind::NotFound => ErrorCode::NoEntry,
                    std::io::ErrorKind::PermissionDenied => ErrorCode::NotPermitted,
                    std::io::ErrorKind::AlreadyExists => ErrorCode::Exist,
                    std::io::ErrorKind::InvalidInput => ErrorCode::Invalid,
                    _ => ErrorCode::Io,
                }
            }
        }
    }
}

impl From<std::io::Error> for ErrorCode {
    fn from(err: std::io::Error) -> ErrorCode {
        ErrorCode::from(&err)
    }
}

impl From<Advice> for system_interface::fs::Advice {
    fn from(advice: Advice) -> Self {
        match advice {
            Advice::Normal => Self::Normal,
            Advice::Sequential => Self::Sequential,
            Advice::Random => Self::Random,
            Advice::WillNeed => Self::WillNeed,
            Advice::DontNeed => Self::DontNeed,
            Advice::NoReuse => Self::NoReuse,
        }
    }
}

#[derive(Clone)]
pub enum Descriptor {
    File(File),
    Dir(Dir),
}

impl Descriptor {
    pub fn file(&self) -> Result<&File, ErrorCode> {
        match self {
            Self::File(f) => Ok(f),
            Self::Dir(_) => Err(ErrorCode::BadDescriptor),
        }
    }

    pub fn dir(&self) -> Result<&Dir, ErrorCode> {
        match self {
            Self::Dir(d) => Ok(d),
            Self::File(_) => Err(ErrorCode::NotDirectory),
        }
    }

    pub fn is_file(&self) -> bool {
        match self {
            Self::File(_) => true,
            Self::Dir(_) => false,
        }
    }

    pub fn is_dir(&self) -> bool {
        match self {
            Self::File(_) => false,
            Self::Dir(_) => true,
        }
    }

    async fn get_metadata(&self) -> std::io::Result<cap_std::fs::Metadata> {
        match self {
            Self::File(f) => {
                // No permissions check on metadata: if opened, allowed to stat it
                f.run_blocking(|f| f.metadata()).await
            }
            Self::Dir(d) => {
                // No permissions check on metadata: if opened, allowed to stat it
                d.run_blocking(|d| d.dir_metadata()).await
            }
        }
    }

    async fn advise(self, offset: u64, len: u64, advice: Advice) -> Result<(), ErrorCode> {
        use system_interface::fs::FileIoExt;

        let f = self.file()?;
        f.run_blocking(move |f| f.advise(offset, len, advice.into()))
            .await?;
        Ok(())
    }

    async fn sync_data(self) -> Result<(), ErrorCode> {
        match self {
            Self::File(f) => {
                match f.run_blocking(|f| f.sync_data()).await {
                    Ok(()) => Ok(()),
                    // On windows, `sync_data` uses `FileFlushBuffers` which fails with
                    // `ERROR_ACCESS_DENIED` if the file is not upen for writing. Ignore
                    // this error, for POSIX compatibility.
                    #[cfg(windows)]
                    Err(err)
                        if err.raw_os_error()
                            == Some(windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED as _) =>
                    {
                        Ok(())
                    }
                    Err(err) => Err(err.into()),
                }
            }
            Self::Dir(d) => {
                d.run_blocking(|d| {
                    let d = d.open(std::path::Component::CurDir)?;
                    d.sync_data()?;
                    Ok(())
                })
                .await
            }
        }
    }

    async fn get_flags(self) -> Result<DescriptorFlags, ErrorCode> {
        use system_interface::fs::{FdFlags, GetSetFdFlags};

        fn get_from_fdflags(flags: FdFlags) -> DescriptorFlags {
            let mut out = DescriptorFlags::empty();
            if flags.contains(FdFlags::DSYNC) {
                out |= DescriptorFlags::REQUESTED_WRITE_SYNC;
            }
            if flags.contains(FdFlags::RSYNC) {
                out |= DescriptorFlags::DATA_INTEGRITY_SYNC;
            }
            if flags.contains(FdFlags::SYNC) {
                out |= DescriptorFlags::FILE_INTEGRITY_SYNC;
            }
            out
        }
        match self {
            Self::File(f) => {
                let flags = f.run_blocking(|f| f.get_fd_flags()).await?;
                let mut flags = get_from_fdflags(flags);
                if f.open_mode.contains(OpenMode::READ) {
                    flags |= DescriptorFlags::READ;
                }
                if f.open_mode.contains(OpenMode::WRITE) {
                    flags |= DescriptorFlags::WRITE;
                }
                Ok(flags)
            }
            Self::Dir(d) => {
                let flags = d.run_blocking(|d| d.get_fd_flags()).await?;
                let mut flags = get_from_fdflags(flags);
                if d.open_mode.contains(OpenMode::READ) {
                    flags |= DescriptorFlags::READ;
                }
                if d.open_mode.contains(OpenMode::WRITE) {
                    flags |= DescriptorFlags::MUTATE_DIRECTORY;
                }
                Ok(flags)
            }
        }
    }

    async fn get_type(self) -> Result<DescriptorType, ErrorCode> {
        match self {
            Self::File(f) => {
                let meta = f.run_blocking(|f| f.metadata()).await?;
                Ok(meta.file_type().into())
            }
            Self::Dir(_) => Ok(DescriptorType::Directory),
        }
    }

    async fn set_size(self, size: u64) -> Result<(), ErrorCode> {
        let f = self.file()?;
        if !f.perms.contains(FilePerms::WRITE) {
            return Err(ErrorCode::NotPermitted);
        }
        f.run_blocking(move |f| f.set_len(size)).await?;
        Ok(())
    }

    async fn set_times(self, atim: NewTimestamp, mtim: NewTimestamp) -> Result<(), ErrorCode> {
        use fs_set_times::SetTimes as _;

        match self {
            Self::File(f) => {
                if !f.perms.contains(FilePerms::WRITE) {
                    return Err(ErrorCode::NotPermitted);
                }
                let atim = systemtimespec_from(atim)?;
                let mtim = systemtimespec_from(mtim)?;
                f.run_blocking(|f| f.set_times(atim, mtim)).await?;
                Ok(())
            }
            Self::Dir(d) => {
                if !d.perms.contains(DirPerms::MUTATE) {
                    return Err(ErrorCode::NotPermitted);
                }
                let atim = systemtimespec_from(atim)?;
                let mtim = systemtimespec_from(mtim)?;
                d.run_blocking(|d| d.set_times(atim, mtim)).await?;
                Ok(())
            }
        }
    }

    async fn sync(self) -> Result<(), ErrorCode> {
        match self {
            Self::File(f) => {
                match f.run_blocking(|f| f.sync_all()).await {
                    Ok(()) => Ok(()),
                    // On windows, `sync_data` uses `FileFlushBuffers` which fails with
                    // `ERROR_ACCESS_DENIED` if the file is not upen for writing. Ignore
                    // this error, for POSIX compatibility.
                    #[cfg(windows)]
                    Err(err)
                        if err.raw_os_error()
                            == Some(windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED as _) =>
                    {
                        Ok(())
                    }
                    Err(err) => Err(err.into()),
                }
            }
            Self::Dir(d) => {
                d.run_blocking(|d| {
                    let d = d.open(std::path::Component::CurDir)?;
                    d.sync_all()?;
                    Ok(())
                })
                .await
            }
        }
    }

    async fn create_directory_at(self, path: String) -> Result<(), ErrorCode> {
        let d = self.dir()?;
        if !d.perms.contains(DirPerms::MUTATE) {
            return Err(ErrorCode::NotPermitted);
        }
        d.run_blocking(move |d| d.create_dir(&path)).await?;
        Ok(())
    }

    async fn stat(self) -> Result<DescriptorStat, ErrorCode> {
        match self {
            Self::File(f) => {
                // No permissions check on stat: if opened, allowed to stat it
                let meta = f.run_blocking(|f| f.metadata()).await?;
                Ok(meta.into())
            }
            Self::Dir(d) => {
                // No permissions check on stat: if opened, allowed to stat it
                let meta = d.run_blocking(|d| d.dir_metadata()).await?;
                Ok(meta.into())
            }
        }
    }

    async fn stat_at(
        self,
        path_flags: PathFlags,
        path: String,
    ) -> Result<DescriptorStat, ErrorCode> {
        let d = self.dir()?;
        if !d.perms.contains(DirPerms::READ) {
            return Err(ErrorCode::NotPermitted);
        }

        let meta = if path_flags.contains(PathFlags::SYMLINK_FOLLOW) {
            d.run_blocking(move |d| d.metadata(&path)).await?
        } else {
            d.run_blocking(move |d| d.symlink_metadata(&path)).await?
        };
        Ok(meta.into())
    }

    async fn set_times_at(
        self,
        path_flags: PathFlags,
        path: String,
        atim: NewTimestamp,
        mtim: NewTimestamp,
    ) -> Result<(), ErrorCode> {
        use cap_fs_ext::DirExt as _;

        let d = self.dir()?;
        if !d.perms.contains(DirPerms::MUTATE) {
            return Err(ErrorCode::NotPermitted);
        }
        let atim = systemtimespec_from(atim)?;
        let mtim = systemtimespec_from(mtim)?;
        if path_flags.contains(PathFlags::SYMLINK_FOLLOW) {
            d.run_blocking(move |d| {
                d.set_times(
                    &path,
                    atim.map(cap_fs_ext::SystemTimeSpec::from_std),
                    mtim.map(cap_fs_ext::SystemTimeSpec::from_std),
                )
            })
            .await?;
        } else {
            d.run_blocking(move |d| {
                d.set_symlink_times(
                    &path,
                    atim.map(cap_fs_ext::SystemTimeSpec::from_std),
                    mtim.map(cap_fs_ext::SystemTimeSpec::from_std),
                )
            })
            .await?;
        }
        Ok(())
    }

    async fn link_at(
        self,
        old_path_flags: PathFlags,
        old_path: String,
        new_descriptor: Self,
        new_path: String,
    ) -> Result<(), ErrorCode> {
        let old_dir = self.dir()?;
        if !old_dir.perms.contains(DirPerms::MUTATE) {
            return Err(ErrorCode::NotPermitted);
        }
        let new_dir = new_descriptor.dir()?;
        if !new_dir.perms.contains(DirPerms::MUTATE) {
            return Err(ErrorCode::NotPermitted);
        }
        if old_path_flags.contains(PathFlags::SYMLINK_FOLLOW) {
            return Err(ErrorCode::Invalid);
        }
        let new_dir_handle = Arc::clone(&new_dir.dir);
        old_dir
            .run_blocking(move |d| d.hard_link(&old_path, &new_dir_handle, &new_path))
            .await?;
        Ok(())
    }

    async fn open_at(
        self,
        path_flags: PathFlags,
        path: String,
        oflags: OpenFlags,
        flags: DescriptorFlags,
        allow_blocking_current_thread: bool,
    ) -> Result<Self, ErrorCode> {
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
        use system_interface::fs::{FdFlags, GetSetFdFlags};

        let d = self.dir()?;
        if !d.perms.contains(DirPerms::READ) {
            return Err(ErrorCode::NotPermitted);
        }

        if !d.perms.contains(DirPerms::MUTATE) {
            if oflags.contains(OpenFlags::CREATE) || oflags.contains(OpenFlags::TRUNCATE) {
                return Err(ErrorCode::NotPermitted);
            }
            if flags.contains(DescriptorFlags::WRITE) {
                return Err(ErrorCode::NotPermitted);
            }
        }

        // Track whether we are creating file, for permission check:
        let mut create = false;
        // Track open mode, for permission check and recording in created descriptor:
        let mut open_mode = OpenMode::empty();
        // Construct the OpenOptions to give the OS:
        let mut opts = cap_std::fs::OpenOptions::new();
        opts.maybe_dir(true);

        if oflags.contains(OpenFlags::CREATE) {
            if oflags.contains(OpenFlags::EXCLUSIVE) {
                opts.create_new(true);
            } else {
                opts.create(true);
            }
            create = true;
            opts.write(true);
            open_mode |= OpenMode::WRITE;
        }

        if oflags.contains(OpenFlags::TRUNCATE) {
            opts.truncate(true).write(true);
        }
        if flags.contains(DescriptorFlags::READ) {
            opts.read(true);
            open_mode |= OpenMode::READ;
        }
        if flags.contains(DescriptorFlags::WRITE) {
            opts.write(true);
            open_mode |= OpenMode::WRITE;
        } else {
            // If not opened write, open read. This way the OS lets us open
            // the file, but we can use perms to reject use of the file later.
            opts.read(true);
            open_mode |= OpenMode::READ;
        }
        if path_flags.contains(PathFlags::SYMLINK_FOLLOW) {
            opts.follow(FollowSymlinks::Yes);
        } else {
            opts.follow(FollowSymlinks::No);
        }

        // These flags are not yet supported in cap-std:
        if flags.contains(DescriptorFlags::FILE_INTEGRITY_SYNC)
            || flags.contains(DescriptorFlags::DATA_INTEGRITY_SYNC)
            || flags.contains(DescriptorFlags::REQUESTED_WRITE_SYNC)
        {
            return Err(ErrorCode::Unsupported);
        }

        if oflags.contains(OpenFlags::DIRECTORY) {
            if oflags.contains(OpenFlags::CREATE)
                || oflags.contains(OpenFlags::EXCLUSIVE)
                || oflags.contains(OpenFlags::TRUNCATE)
            {
                return Err(ErrorCode::Invalid);
            }
        }

        // Now enforce this WasiCtx's permissions before letting the OS have
        // its shot:
        if !d.perms.contains(DirPerms::MUTATE) && create {
            return Err(ErrorCode::NotPermitted);
        }
        if !d.file_perms.contains(FilePerms::WRITE) && open_mode.contains(OpenMode::WRITE) {
            return Err(ErrorCode::NotPermitted);
        }

        // Represents each possible outcome from the spawn_blocking operation.
        // This makes sure we don't have to give spawn_blocking any way to
        // manipulate the table.
        enum OpenResult {
            Dir(cap_std::fs::Dir),
            File(cap_std::fs::File),
            NotDir,
        }

        let opened = d
            .run_blocking::<_, std::io::Result<OpenResult>>(move |d| {
                let mut opened = d.open_with(&path, &opts)?;
                if opened.metadata()?.is_dir() {
                    Ok(OpenResult::Dir(cap_std::fs::Dir::from_std_file(
                        opened.into_std(),
                    )))
                } else if oflags.contains(OpenFlags::DIRECTORY) {
                    Ok(OpenResult::NotDir)
                } else {
                    // FIXME cap-std needs a nonblocking open option so that files reads and writes
                    // are nonblocking. Instead we set it after opening here:
                    let set_fd_flags = opened.new_set_fd_flags(FdFlags::NONBLOCK)?;
                    opened.set_fd_flags(set_fd_flags)?;
                    Ok(OpenResult::File(opened))
                }
            })
            .await?;

        match opened {
            OpenResult::Dir(dir) => Ok(Self::Dir(Dir::new(
                dir,
                d.perms,
                d.file_perms,
                open_mode,
                allow_blocking_current_thread,
            ))),

            OpenResult::File(file) => Ok(Self::File(File::new(
                file,
                d.file_perms,
                open_mode,
                allow_blocking_current_thread,
            ))),

            OpenResult::NotDir => Err(ErrorCode::NotDirectory),
        }
    }

    async fn readlink_at(self, path: String) -> Result<String, ErrorCode> {
        let d = self.dir()?;
        if !d.perms.contains(DirPerms::READ) {
            return Err(ErrorCode::NotPermitted);
        }
        let link = d.run_blocking(move |d| d.read_link(&path)).await?;
        link.into_os_string()
            .into_string()
            .or(Err(ErrorCode::IllegalByteSequence))
    }

    async fn remove_directory_at(self, path: String) -> Result<(), ErrorCode> {
        let d = self.dir()?;
        if !d.perms.contains(DirPerms::MUTATE) {
            return Err(ErrorCode::NotPermitted);
        }
        d.run_blocking(move |d| d.remove_dir(&path)).await?;
        Ok(())
    }

    async fn rename_at(
        self,
        old_path: String,
        new_fd: Self,
        new_path: String,
    ) -> Result<(), ErrorCode> {
        let old_dir = self.dir()?;
        if !old_dir.perms.contains(DirPerms::MUTATE) {
            return Err(ErrorCode::NotPermitted);
        }
        let new_dir = new_fd.dir()?;
        if !new_dir.perms.contains(DirPerms::MUTATE) {
            return Err(ErrorCode::NotPermitted);
        }
        let new_dir_handle = Arc::clone(&new_dir.dir);
        old_dir
            .run_blocking(move |d| d.rename(&old_path, &new_dir_handle, &new_path))
            .await?;
        Ok(())
    }

    async fn symlink_at(self, src_path: String, dest_path: String) -> Result<(), ErrorCode> {
        // On windows, Dir.symlink is provided by DirExt
        #[cfg(windows)]
        use cap_fs_ext::DirExt;

        let d = self.dir()?;
        if !d.perms.contains(DirPerms::MUTATE) {
            return Err(ErrorCode::NotPermitted);
        }
        d.run_blocking(move |d| d.symlink(&src_path, &dest_path))
            .await?;
        Ok(())
    }

    async fn unlink_file_at(self, path: String) -> Result<(), ErrorCode> {
        use cap_fs_ext::DirExt;

        let d = self.dir()?;
        if !d.perms.contains(DirPerms::MUTATE) {
            return Err(ErrorCode::NotPermitted);
        }
        d.run_blocking(move |d| d.remove_file_or_symlink(&path))
            .await?;
        Ok(())
    }

    async fn is_same_object(self, other: Self) -> wasmtime::Result<bool> {
        use cap_fs_ext::MetadataExt;
        let meta_a = self.get_metadata().await?;
        let meta_b = other.get_metadata().await?;
        if meta_a.dev() == meta_b.dev() && meta_a.ino() == meta_b.ino() {
            // MetadataHashValue does not derive eq, so use a pair of
            // comparisons to check equality:
            debug_assert_eq!(
                MetadataHashValue::from(&meta_a).upper,
                MetadataHashValue::from(&meta_b).upper,
            );
            debug_assert_eq!(
                MetadataHashValue::from(&meta_a).lower,
                MetadataHashValue::from(&meta_b).lower,
            );
            Ok(true)
        } else {
            // Hash collisions are possible, so don't assert the negative here
            Ok(false)
        }
    }
    async fn metadata_hash(self) -> Result<MetadataHashValue, ErrorCode> {
        let meta = self.get_metadata().await?;
        Ok(MetadataHashValue::from(&meta))
    }
    async fn metadata_hash_at(
        self,
        path_flags: PathFlags,
        path: String,
    ) -> Result<MetadataHashValue, ErrorCode> {
        let d = self.dir()?;
        // No permissions check on metadata: if dir opened, allowed to stat it
        let meta = d
            .run_blocking(move |d| {
                if path_flags.contains(PathFlags::SYMLINK_FOLLOW) {
                    d.metadata(path)
                } else {
                    d.symlink_metadata(path)
                }
            })
            .await?;
        Ok(MetadataHashValue::from(&meta))
    }
}

#[derive(Clone)]
pub struct File {
    /// The operating system File this struct is mediating access to.
    ///
    /// Wrapped in an Arc because the same underlying file is used for
    /// implementing the stream types. A copy is also needed for
    /// [`spawn_blocking`].
    ///
    /// [`spawn_blocking`]: Self::spawn_blocking
    file: Arc<cap_std::fs::File>,
    /// Permissions to enforce on access to the file. These permissions are
    /// specified by a user of the `crate::WasiCtxBuilder`, and are
    /// enforced prior to any enforced by the underlying operating system.
    perms: FilePerms,
    /// The mode the file was opened under: bits for reading, and writing.
    /// Required to correctly report the DescriptorFlags, because cap-std
    /// doesn't presently provide a cross-platform equivalent of reading the
    /// oflags back out using fcntl.
    open_mode: OpenMode,

    tasks: Arc<std::sync::Mutex<TaskTable>>,

    allow_blocking_current_thread: bool,
}

impl File {
    pub fn new(
        file: cap_std::fs::File,
        perms: FilePerms,
        open_mode: OpenMode,
        allow_blocking_current_thread: bool,
    ) -> Self {
        Self {
            file: Arc::new(file),
            perms,
            open_mode,
            tasks: Arc::default(),
            allow_blocking_current_thread,
        }
    }

    /// Execute the blocking `body` function.
    ///
    /// Depending on how the WasiCtx was configured, the body may either be:
    /// - Executed directly on the current thread. In this case the `async`
    ///   signature of this method is effectively a lie and the returned
    ///   Future will always be immediately Ready. Or:
    /// - Spawned on a background thread using [`tokio::task::spawn_blocking`]
    ///   and immediately awaited.
    ///
    /// Intentionally blocking the executor thread might seem unorthodox, but is
    /// not actually a problem for specific workloads. See:
    /// - [`crate::WasiCtxBuilder::allow_blocking_current_thread`]
    /// - [Poor performance of wasmtime file I/O maybe because tokio](https://github.com/bytecodealliance/wasmtime/issues/7973)
    /// - [Implement opt-in for enabling WASI to block the current thread](https://github.com/bytecodealliance/wasmtime/pull/8190)
    pub(crate) async fn run_blocking<F, R>(&self, body: F) -> R
    where
        F: FnOnce(&cap_std::fs::File) -> R + Send + 'static,
        R: Send + 'static,
    {
        match self.as_blocking_file() {
            Some(file) => body(file),
            None => self.spawn_blocking(body).await,
        }
    }

    pub(crate) fn spawn_blocking<F, R>(&self, body: F) -> AbortOnDropJoinHandle<R>
    where
        F: FnOnce(&cap_std::fs::File) -> R + Send + 'static,
        R: Send + 'static,
    {
        let f = self.file.clone();
        spawn_blocking(move || body(&f))
    }

    /// Returns `Some` when the current thread is allowed to block in filesystem
    /// operations, and otherwise returns `None` to indicate that
    /// `spawn_blocking` must be used.
    pub(crate) fn as_blocking_file(&self) -> Option<&cap_std::fs::File> {
        if self.allow_blocking_current_thread {
            Some(&self.file)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct Dir {
    /// The operating system file descriptor this struct is mediating access
    /// to.
    ///
    /// Wrapped in an Arc because a copy is needed for [`spawn_blocking`].
    ///
    /// [`spawn_blocking`]: Self::spawn_blocking
    dir: Arc<cap_std::fs::Dir>,
    /// Permissions to enforce on access to this directory. These permissions
    /// are specified by a user of the `crate::WasiCtxBuilder`, and
    /// are enforced prior to any enforced by the underlying operating system.
    ///
    /// These permissions are also enforced on any directories opened under
    /// this directory.
    perms: DirPerms,
    /// Permissions to enforce on any files opened under this directory.
    file_perms: FilePerms,
    /// The mode the directory was opened under: bits for reading, and writing.
    /// Required to correctly report the DescriptorFlags, because cap-std
    /// doesn't presently provide a cross-platform equivalent of reading the
    /// oflags back out using fcntl.
    open_mode: OpenMode,

    tasks: Arc<std::sync::Mutex<TaskTable>>,

    allow_blocking_current_thread: bool,
}

impl Dir {
    pub fn new(
        dir: cap_std::fs::Dir,
        perms: DirPerms,
        file_perms: FilePerms,
        open_mode: OpenMode,
        allow_blocking_current_thread: bool,
    ) -> Self {
        Dir {
            dir: Arc::new(dir),
            perms,
            file_perms,
            open_mode,
            tasks: Arc::default(),
            allow_blocking_current_thread,
        }
    }

    /// Execute the blocking `body` function.
    ///
    /// Depending on how the WasiCtx was configured, the body may either be:
    /// - Executed directly on the current thread. In this case the `async`
    ///   signature of this method is effectively a lie and the returned
    ///   Future will always be immediately Ready. Or:
    /// - Spawned on a background thread using [`tokio::task::spawn_blocking`]
    ///   and immediately awaited.
    ///
    /// Intentionally blocking the executor thread might seem unorthodox, but is
    /// not actually a problem for specific workloads. See:
    /// - [`crate::WasiCtxBuilder::allow_blocking_current_thread`]
    /// - [Poor performance of wasmtime file I/O maybe because tokio](https://github.com/bytecodealliance/wasmtime/issues/7973)
    /// - [Implement opt-in for enabling WASI to block the current thread](https://github.com/bytecodealliance/wasmtime/pull/8190)
    pub(crate) async fn run_blocking<F, R>(&self, body: F) -> R
    where
        F: FnOnce(&cap_std::fs::Dir) -> R + Send + 'static,
        R: Send + 'static,
    {
        if self.allow_blocking_current_thread {
            body(&self.dir)
        } else {
            let d = self.dir.clone();
            spawn_blocking(move || body(&d)).await
        }
    }
}
