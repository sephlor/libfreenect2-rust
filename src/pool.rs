use std::sync::Arc;

use cuda_async::device_context::{init_device_contexts, with_device};
use cuda_core::{MemPool, Stream, sys};

use crate::error;

#[derive(Clone)]
pub(crate) struct HostPool {
    pub(crate) mempool: Arc<MemPool>,
    pub(crate) stream: Arc<Stream>,
}

impl HostPool {
    pub(crate) fn new() -> error::Result<Self> {
        init_device_contexts(0, 1).expect("init failed (requires GPU)");

        // SAFETY: CUmemPoolProps is a plain C struct; all-zero is a valid starting value.
        let mut props: sys::CUmemPoolProps = unsafe { std::mem::zeroed() };
        props.allocType = sys::CUmemAllocationType_enum_CU_MEM_ALLOCATION_TYPE_PINNED;
        props.handleTypes = sys::CUmemAllocationHandleType_enum_CU_MEM_HANDLE_TYPE_NONE;
        props.location.type_ = sys::CUmemLocationType_enum_CU_MEM_LOCATION_TYPE_HOST;

        let mempool = with_device(0, |device| device.new_mem_pool_with_params(props))??;
        mempool.set_release_threshold(u64::MAX)?;

        let stream = with_device(0, |device| device.new_stream())
            .unwrap()
            .unwrap();

        Ok(Self { mempool, stream })
    }
}
