/// Read data from Region to argument `data`,
/// return `true` if read successfully, or return `false`.
///
/// # Arguments
///
/// * `offset` - Base address offset.
/// * `access_size` - Access size.
type ReadFn = alloc::sync::Arc<dyn Fn(u64, u8) -> axerrno::AxResult<u64> + Send + Sync>;

/// Write `data` to memory,
/// return `true` if write successfully, or return `false`.
///
/// # Arguments
///
/// * `offset` - Base address offset
/// * `access_size` - Access size.
/// * `data` - A u8-type array.
type WriteFn = alloc::sync::Arc<dyn Fn(u64, u8, &[u8]) -> axerrno::AxResult + Send + Sync>;

/// Provide Some operations of `Region`, mainly used by Vm's devices.
#[derive(Clone)]
pub struct RegionOps {
    /// Read data from Region to argument `data`,
    pub read: ReadFn,
    /// Write `data` to memory,
    pub write: WriteFn,
}

#[cfg(target_arch = "x86_64")]
/// Vmexit caused by port io operations.
pub trait PioOps: Send + Sync {
    /// Port range.
    fn port_range(&self) -> core::ops::Range<u16>;
    /// Read operation
    fn read(&mut self, port: u16, access_size: u8) -> axerrno::AxResult<u32>;
    /// Write operation
    fn write(&mut self, port: u16, access_size: u8, value: u32) -> axerrno::AxResult;
}

/// Vmexit caused by mmio operations.
pub trait MmioOps: Send + Sync {
    /// Mmio range.
    fn mmio_range(&self) -> core::ops::Range<u64>;
    /// Read operation
    fn read(&mut self, addr: u64, access_size: u8) -> axerrno::AxResult<u64>;
    /// Write operation
    fn write(&mut self, addr: u64, access_size: u8, value: u64) -> axerrno::AxResult;
}
