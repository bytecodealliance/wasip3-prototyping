#![allow(unused)] // TODO: remove

use wasmtime::component::{FutureReader, Resource, StreamReader};

use crate::p3::bindings::filesystem::types::{
    Advice, Descriptor, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry, ErrorCode,
    Filesize, MetadataHashValue, NewTimestamp, OpenFlags, PathFlags,
};
use crate::p3::bindings::filesystem::{preopens, types};
use crate::p3::filesystem::{WasiFilesystemImpl, WasiFilesystemView};

impl<T> types::Host for WasiFilesystemImpl<T> where T: WasiFilesystemView {}

impl<T> types::HostDescriptor for WasiFilesystemImpl<T>
where
    T: WasiFilesystemView,
{
    fn read_via_stream(
        &mut self,
        descriptor: Resource<Descriptor>,
        offset: Filesize,
    ) -> wasmtime::Result<(StreamReader<u8>, FutureReader<Result<(), ErrorCode>>)> {
        todo!()
    }

    fn write_via_stream(
        &mut self,
        descriptor: Resource<Descriptor>,
        data: StreamReader<u8>,
        offset: Filesize,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn append_via_stream(
        &mut self,
        descriptor: Resource<Descriptor>,
        data: StreamReader<u8>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn advise(
        &mut self,
        descriptor: Resource<Descriptor>,
        offset: Filesize,
        length: Filesize,
        advice: Advice,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn sync_data(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn get_flags(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<DescriptorFlags, ErrorCode>> {
        todo!()
    }

    fn get_type(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<DescriptorType, ErrorCode>> {
        todo!()
    }

    fn set_size(
        &mut self,
        descriptor: Resource<Descriptor>,
        size: Filesize,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn set_times(
        &mut self,
        descriptor: Resource<Descriptor>,
        data_access_timestamp: NewTimestamp,
        data_modification_timestamp: NewTimestamp,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn read_directory(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> wasmtime::Result<(
        StreamReader<DirectoryEntry>,
        FutureReader<Result<(), ErrorCode>>,
    )> {
        todo!()
    }

    fn sync(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn create_directory_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn stat(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<DescriptorStat, ErrorCode>> {
        todo!()
    }

    fn stat_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
    ) -> wasmtime::Result<Result<DescriptorStat, ErrorCode>> {
        todo!()
    }

    fn set_times_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
        data_access_timestamp: NewTimestamp,
        data_modification_timestamp: NewTimestamp,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn link_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        old_path_flags: PathFlags,
        old_path: String,
        new_descriptor: Resource<Descriptor>,
        new_path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn open_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
        open_flags: OpenFlags,
        flags: DescriptorFlags,
    ) -> wasmtime::Result<Result<Resource<Descriptor>, ErrorCode>> {
        todo!()
    }

    fn readlink_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<String, ErrorCode>> {
        todo!()
    }

    fn remove_directory_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn rename_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        old_path: String,
        new_descriptor: Resource<Descriptor>,
        new_path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn symlink_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        old_path: String,
        new_path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn unlink_file_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path: String,
    ) -> wasmtime::Result<Result<(), ErrorCode>> {
        todo!()
    }

    fn is_same_object(
        &mut self,
        descriptor: Resource<Descriptor>,
        other: Resource<Descriptor>,
    ) -> wasmtime::Result<bool> {
        todo!()
    }

    fn metadata_hash(
        &mut self,
        descriptor: Resource<Descriptor>,
    ) -> wasmtime::Result<Result<MetadataHashValue, ErrorCode>> {
        todo!()
    }

    fn metadata_hash_at(
        &mut self,
        descriptor: Resource<Descriptor>,
        path_flags: PathFlags,
        path: String,
    ) -> wasmtime::Result<Result<MetadataHashValue, ErrorCode>> {
        todo!()
    }

    fn drop(&mut self, rep: Resource<Descriptor>) -> wasmtime::Result<()> {
        todo!()
    }
}

impl<T> preopens::Host for WasiFilesystemImpl<T>
where
    T: WasiFilesystemView,
{
    fn get_directories(&mut self) -> wasmtime::Result<Vec<(Resource<Descriptor>, String)>> {
        Ok(vec![])
    }
}
