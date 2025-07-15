use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use system_interface::fs::FileIoExt as _;
use tokio::sync::mpsc;
use wasmtime::component::{
    Accessor, AccessorTask, HostFuture, HostStream, Lower, Resource, ResourceTable,
};

use crate::p3::bindings::filesystem::types::{
    Advice, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry, ErrorCode, Filesize,
    MetadataHashValue, NewTimestamp, OpenFlags, PathFlags,
};
use crate::p3::bindings::filesystem::{preopens, types};
use crate::p3::filesystem::{
    Descriptor, DirPerms, FilePerms, WasiFilesystem, WasiFilesystemImpl, WasiFilesystemView,
};
use crate::p3::{AbortOnDropHandle, IoTask, ResourceView as _, SpawnExt, TaskTable};

fn get_descriptor<'a>(
    table: &'a ResourceTable,
    fd: &'a Resource<Descriptor>,
) -> wasmtime::Result<&'a Descriptor> {
    table
        .get(fd)
        .context("failed to get descriptor resource from table")
}

pub struct ReadTask<T> {
    io: IoTask<T, ErrorCode>,
    id: u32,
    tasks: Arc<std::sync::Mutex<TaskTable>>,
}

impl<T, U, V> AccessorTask<T, U, wasmtime::Result<()>> for ReadTask<V>
where
    U: wasmtime::component::HasData,
    V: Lower + Send + Sync + 'static,
{
    async fn run(self, store: &Accessor<T, U>) -> wasmtime::Result<()> {
        let res = self.io.run(store).await;
        let mut tasks = self.tasks.lock().map_err(|_| anyhow!("lock poisoned"))?;
        tasks.remove(self.id);
        res
    }
}

impl<T> types::Host for WasiFilesystemImpl<T> where T: WasiFilesystemView {}

impl<T> types::HostConcurrent for WasiFilesystem<T> where T: WasiFilesystemView + 'static {}

impl<T> types::HostDescriptorConcurrent for WasiFilesystem<T>
where
    T: WasiFilesystemView + 'static,
{
    async fn read_via_stream<U: 'static>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        mut offset: Filesize,
    ) -> wasmtime::Result<(HostStream<u8>, HostFuture<Result<(), ErrorCode>>)> {
        store.with(|mut view| {
            let instance = view.instance();
            let (data_tx, data_rx) = instance
                .stream::<_, _, Vec<_>>(&mut view)
                .context("failed to create stream")?;
            let (res_tx, res_rx) = instance
                .future(|| unreachable!(), &mut view)
                .context("failed to create future")?;
            let mut binding = view.get();
            let fd = get_descriptor(binding.table(), &fd)?;
            match fd.file() {
                Ok(f) => {
                    let (task_tx, task_rx) = mpsc::channel(1);
                    let f = f.clone();
                    let tasks = Arc::clone(&f.tasks);
                    let task = view.spawn_fn(move |_| async move {
                        while let Ok(tx) = task_tx.reserve().await {
                            match f
                                .spawn_blocking(move |f| {
                                    let mut buf = vec![0; 8096];
                                    loop {
                                        let res = f.read_at(&mut buf, offset);
                                        if let Err(err) = &res {
                                            if err.kind() == std::io::ErrorKind::Interrupted {
                                                // Try again, continue looping
                                                continue;
                                            }
                                        }
                                        return (res, buf);
                                    }
                                })
                                .await
                            {
                                (Ok(0), ..) => break,
                                (Ok(n), mut buf) => {
                                    buf.truncate(n);
                                    let Some(n) =
                                        n.try_into().ok().and_then(|n| offset.checked_add(n))
                                    else {
                                        tx.send(Err(ErrorCode::Overflow));
                                        break;
                                    };
                                    offset = n;
                                    tx.send(Ok(buf));
                                }
                                (Err(err), ..) => {
                                    tx.send(Err(err.into()));
                                    break;
                                }
                            }
                        }
                        Ok(())
                    });
                    let id = {
                        let mut tasks = tasks.lock().map_err(|_| anyhow!("lock poisoned"))?;
                        tasks
                            .push(AbortOnDropHandle(task))
                            .context("failed to push task to table")?
                    };
                    view.spawn(ReadTask {
                        io: IoTask {
                            data: data_tx,
                            result: res_tx,
                            rx: task_rx,
                        },
                        id,
                        tasks,
                    });
                }
                Err(err) => {
                    drop(data_tx);
                    let fut = res_tx.write(Err(err));
                    view.spawn_fn(|_| async {
                        fut.await;
                        Ok(())
                    });
                }
            }
            Ok((data_rx.into(), res_rx.into()))
        })
    }

    async fn write_via_stream<U: 'static>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        data: HostStream<u8>,
        mut offset: Filesize,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let mut buf = Vec::with_capacity(8096);
        let (fd, fut) = store.with(|mut view| {
            let data = data.into_reader::<Vec<u8>>(&mut view);
            let fut = data.read(buf);
            let fd = get_descriptor(view.get().table(), &fd)?.clone();
            anyhow::Ok((fd.clone(), fut))
        })?;
        let f = match fd.file() {
            Ok(f) => f,
            Err(err) => return Ok(Err(err)),
        };
        if !f.perms.contains(FilePerms::WRITE) {
            return Ok(Err(types::ErrorCode::BadDescriptor));
        }
        let mut fut = fut;
        loop {
            let (Some(tail), buf_again) = fut.await else {
                return Ok(Ok(()));
            };
            match f
                .spawn_blocking(move |f| {
                    let mut buf = buf_again.as_slice();
                    while !buf.is_empty() {
                        let n = f.write_at(buf, offset)?;
                        buf = &buf[n..];
                        let n = n.try_into().or(Err(ErrorCode::Overflow))?;
                        offset = offset.checked_add(n).ok_or(ErrorCode::Overflow)?;
                    }
                    Ok((offset, buf_again))
                })
                .await
            {
                Ok((n, buf_again)) => {
                    offset = n;
                    buf = buf_again;
                    buf.clear();
                }
                Err(err) => return Ok(Err(err)),
            }
            fut = tail.read(buf);
        }
    }

    async fn append_via_stream<U: 'static>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        data: HostStream<u8>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let mut buf = Vec::with_capacity(8096);
        let (fd, fut) = store.with(|mut view| {
            let data = data.into_reader::<Vec<u8>>(&mut view);
            let fut = data.read(buf);
            let fd = get_descriptor(view.get().table(), &fd)?.clone();
            anyhow::Ok((fd, fut))
        })?;
        let f = match fd.file() {
            Ok(f) => f,
            Err(err) => return Ok(Err(err)),
        };
        if !f.perms.contains(FilePerms::WRITE) {
            return Ok(Err(types::ErrorCode::BadDescriptor));
        }
        let mut fut = fut;
        loop {
            let (Some(tail), buf_again) = fut.await else {
                return Ok(Ok(()));
            };
            match f
                .spawn_blocking(move |f| {
                    let mut buf = buf_again.as_slice();
                    loop {
                        let n = f.append(buf)?;
                        if buf.len() == n {
                            return Ok(buf_again);
                        }
                        buf = &buf[n..];
                    }
                })
                .await
            {
                Ok(buf_again) => {
                    buf = buf_again;
                    buf.clear();
                }
                Err(err) => return Ok(Err(err)),
            }
            fut = tail.read(buf)
        }
    }

    async fn advise<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        offset: Filesize,
        length: Filesize,
        advice: Advice,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd)
                .map(|fd| fd.clone().advise(offset, length, advice))
        })?;
        Ok(fut.await)
    }

    async fn sync_data<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().sync_data())
        })?;
        Ok(fut.await)
    }

    async fn get_flags<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<DescriptorFlags, ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().get_flags())
        })?;
        Ok(fut.await)
    }

    async fn get_type<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<DescriptorType, ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().get_type())
        })?;
        Ok(fut.await)
    }

    async fn set_size<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        size: Filesize,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().set_size(size))
        })?;
        Ok(fut.await)
    }

    async fn set_times<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        data_access_timestamp: NewTimestamp,
        data_modification_timestamp: NewTimestamp,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| {
                fd.clone()
                    .set_times(data_access_timestamp, data_modification_timestamp)
            })
        })?;
        Ok(fut.await)
    }

    async fn read_directory<U: 'static>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> wasmtime::Result<(
        HostStream<DirectoryEntry>,
        HostFuture<Result<(), ErrorCode>>,
    )> {
        store.with(|mut view| {
            let instance = view.instance();
            let (data_tx, data_rx) = instance
                .stream::<_, _, Vec<_>>(&mut view)
                .context("failed to create stream")?;
            let (res_tx, res_rx) = instance
                .future(|| unreachable!(), &mut view)
                .context("failed to create future")?;
            let mut binding = view.get();
            let fd = get_descriptor(binding.table(), &fd)?;
            match fd.dir().and_then(|d| {
                if !d.perms.contains(DirPerms::READ) {
                    Err(ErrorCode::NotPermitted)
                } else {
                    Ok(d)
                }
            }) {
                Ok(d) => {
                    let d = d.clone();
                    let tasks = Arc::clone(&d.tasks);
                    let (task_tx, task_rx) = mpsc::channel(1);
                    let task = view.spawn_fn(|_| async move {
                        match d.run_blocking(cap_std::fs::Dir::entries).await {
                            Ok(mut entries) => {
                                while let Ok(tx) = task_tx.reserve().await {
                                    match d
                                        .run_blocking(|_| match entries.next()? {
                                            Ok(entry) => {
                                                let meta = match entry.metadata() {
                                                    Ok(meta) => meta,
                                                    Err(err) => return Some(Err(err.into())),
                                                };
                                                let Ok(name) = entry.file_name().into_string()
                                                else {
                                                    return Some(Err(
                                                        ErrorCode::IllegalByteSequence,
                                                    ));
                                                };
                                                Some(Ok((
                                                    Some(DirectoryEntry {
                                                        type_: meta.file_type().into(),
                                                        name,
                                                    }),
                                                    entries,
                                                )))
                                            }
                                            Err(err) => {
                                                // On windows, filter out files like `C:\DumpStack.log.tmp` which we
                                                // can't get full metadata for.
                                                #[cfg(windows)]
                                                {
                                                    use windows_sys::Win32::Foundation::{
                                                        ERROR_ACCESS_DENIED,
                                                        ERROR_SHARING_VIOLATION,
                                                    };
                                                    if err.raw_os_error()
                                                        == Some(ERROR_SHARING_VIOLATION as i32)
                                                        || err.raw_os_error()
                                                            == Some(ERROR_ACCESS_DENIED as i32)
                                                    {
                                                        return Some(Ok((None, entries)));
                                                    }
                                                }
                                                Some(Err(err.into()))
                                            }
                                        })
                                        .await
                                    {
                                        None => break,
                                        Some(Ok((entry, tail))) => {
                                            if let Some(entry) = entry {
                                                tx.send(Ok(vec![entry]));
                                            }
                                            entries = tail;
                                        }
                                        Some(Err(err)) => {
                                            tx.send(Err(err));
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                _ = task_tx.send(Err(err.into())).await;
                            }
                        }
                        Ok(())
                    });
                    let id = {
                        let mut tasks = tasks.lock().map_err(|_| anyhow!("lock poisoned"))?;
                        tasks
                            .push(AbortOnDropHandle(task))
                            .context("failed to push task to table")?
                    };
                    view.spawn(ReadTask {
                        io: IoTask {
                            data: data_tx,
                            result: res_tx,
                            rx: task_rx,
                        },
                        id,
                        tasks,
                    });
                }
                Err(err) => {
                    drop(data_tx);
                    let fut = res_tx.write(Err(err));
                    view.spawn_fn(|_| async {
                        fut.await;
                        Ok(())
                    });
                }
            }
            Ok((data_rx.into(), res_rx.into()))
        })
    }

    async fn sync<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store
            .with(|mut view| get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().sync()))?;
        Ok(fut.await)
    }

    async fn create_directory_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().create_directory_at(path))
        })?;
        Ok(fut.await)
    }

    async fn stat<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<DescriptorStat, ErrorCode>> {
        let fut = store
            .with(|mut view| get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().stat()))?;
        Ok(fut.await)
    }

    async fn stat_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
    ) -> wasmtime::Result<Result<DescriptorStat, ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().stat_at(path_flags, path))
        })?;
        Ok(fut.await)
    }

    async fn set_times_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
        data_access_timestamp: NewTimestamp,
        data_modification_timestamp: NewTimestamp,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| {
                fd.clone().set_times_at(
                    path_flags,
                    path,
                    data_access_timestamp,
                    data_modification_timestamp,
                )
            })
        })?;
        Ok(fut.await)
    }

    async fn link_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        old_path_flags: PathFlags,
        old_path: String,
        new_fd: Resource<Descriptor>,
        new_path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            let new_fd = get_descriptor(view.get().table(), &new_fd).cloned()?;
            get_descriptor(view.get().table(), &fd).map(|fd| {
                fd.clone()
                    .link_at(old_path_flags, old_path, new_fd, new_path)
            })
        })?;
        Ok(fut.await)
    }

    async fn open_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
        open_flags: OpenFlags,
        flags: DescriptorFlags,
    ) -> wasmtime::Result<Result<Resource<Descriptor>, ErrorCode>> {
        let fut = store.with(|mut view| {
            let allow_blocking_current_thread =
                view.get().filesystem().allow_blocking_current_thread;
            get_descriptor(view.get().table(), &fd).map(|fd| {
                fd.clone().open_at(
                    path_flags,
                    path,
                    open_flags,
                    flags,
                    allow_blocking_current_thread,
                )
            })
        })?;
        match fut.await {
            Ok(fd) => store.with(|mut view| {
                let fd = view
                    .get()
                    .table()
                    .push(fd)
                    .context("failed to push descriptor resource to table")?;
                Ok(Ok(fd))
            }),
            Err(err) => Ok(Err(err)),
        }
    }

    async fn readlink_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<String, ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().readlink_at(path))
        })?;
        Ok(fut.await)
    }

    async fn remove_directory_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().remove_directory_at(path))
        })?;
        Ok(fut.await)
    }

    async fn rename_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        old_path: String,
        new_fd: Resource<Descriptor>,
        new_path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            let new_fd = get_descriptor(view.get().table(), &new_fd).cloned()?;
            get_descriptor(view.get().table(), &fd)
                .map(|fd| fd.clone().rename_at(old_path, new_fd, new_path))
        })?;
        Ok(fut.await)
    }

    async fn symlink_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        old_path: String,
        new_path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd)
                .map(|fd| fd.clone().symlink_at(old_path, new_path))
        })?;
        Ok(fut.await)
    }

    async fn unlink_file_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().unlink_file_at(path))
        })?;
        Ok(fut.await)
    }

    async fn is_same_object<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        other: Resource<Descriptor>,
    ) -> wasmtime::Result<bool> {
        let fut = store.with(|mut view| {
            let other = get_descriptor(view.get().table(), &other).cloned()?;
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().is_same_object(other))
        })?;
        fut.await
    }

    async fn metadata_hash<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<MetadataHashValue, ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd).map(|fd| fd.clone().metadata_hash())
        })?;
        Ok(fut.await)
    }

    async fn metadata_hash_at<U>(
        store: &Accessor<U, Self>,
        fd: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
    ) -> wasmtime::Result<Result<MetadataHashValue, ErrorCode>> {
        let fut = store.with(|mut view| {
            get_descriptor(view.get().table(), &fd)
                .map(|fd| fd.clone().metadata_hash_at(path_flags, path))
        })?;
        Ok(fut.await)
    }
}

impl<T> types::HostDescriptor for WasiFilesystemImpl<T>
where
    T: WasiFilesystemView,
{
    fn drop(&mut self, rep: Resource<Descriptor>) -> wasmtime::Result<()> {
        self.table()
            .delete(rep)
            .context("failed to delete descriptor resource from table")?;
        Ok(())
    }
}

impl<T> preopens::Host for WasiFilesystemImpl<T>
where
    T: WasiFilesystemView,
{
    fn get_directories(&mut self) -> wasmtime::Result<Vec<(Resource<Descriptor>, String)>> {
        let preopens = self.filesystem().preopens.clone();
        let mut results = Vec::with_capacity(preopens.len());
        for (dir, name) in preopens {
            let fd = self
                .table()
                .push(Descriptor::Dir(dir))
                .with_context(|| format!("failed to push preopen {name}"))?;
            results.push((fd, name));
        }
        Ok(results)
    }
}
