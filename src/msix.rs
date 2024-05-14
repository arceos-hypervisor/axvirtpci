use alloc::sync::Arc;
use alloc::vec::Vec;
use bit_field::BitField;
use core::cmp::max;
use core::sync::atomic::{AtomicU16, Ordering};
use spin::Mutex;

use crate::config::{CapId, RegionType, MINIMUM_BAR_SIZE_FOR_MMIO};
use crate::util::num_ops::{ranges_overlap, round_up};
use crate::{le_read_u16, le_read_u64, le_write_u16, le_write_u32, le_write_u64, PciDevBase};
use crate::{BarAllocTrait, MsiIrqManager};
use hypercraft::{HyperError, HyperResult, RegionOps};

pub const MSIX_TABLE_ENTRY_SIZE: u16 = 16;
pub const MSIX_TABLE_SIZE_MAX: u16 = 0x7ff;
const MSIX_TABLE_VEC_CTL: u16 = 0x0c;
const MSIX_TABLE_MASK_BIT: u8 = 0x01;
pub const MSIX_TABLE_BIR: u16 = 0x07;
pub const MSIX_TABLE_OFFSET: u32 = 0xffff_fff8;
const MSIX_MSG_DATA: u16 = 0x08;

pub const MSIX_CAP_CONTROL: u8 = 0x02;
pub const MSIX_CAP_ENABLE: u16 = 0x8000;
pub const MSIX_CAP_FUNC_MASK: u16 = 0x4000;
pub const MSIX_CAP_SIZE: u8 = 12;
pub const MSIX_CAP_ID: u8 = 0x11;
pub const MSIX_CAP_TABLE: u8 = 0x04;
pub const MSI_ADDR_BASE: u32 = 0xfee;
pub const MSI_ADDR_DESTMODE_PHYS: u32 = 0x0;

const MSIX_CAP_PBA: u8 = 0x08;

/// Basic data for msi vector.
// #[derive(Copy, Clone, Default)]
// pub struct MsiVector {
//     pub msg_addr_lo: u32,
//     pub msg_addr_hi: u32,
//     pub msg_data: u32,
//     pub masked: bool,
// }

#[repr(C)]
pub struct MsiAddrReg {
    full: u64,
}

impl From<u64> for MsiAddrReg {
    fn from(item: u64) -> Self {
        MsiAddrReg { full: item }
    }
}

impl From<MsiAddrReg> for u64 {
    fn from(item: MsiAddrReg) -> Self {
        item.full
    }
}

impl MsiAddrReg {
    /// rsvd_1: 0:1
    pub fn rsvd_1(&self) -> u32 {
        self.full.get_bits(0..2) as u32
    }
    /// dest_mode: 2
    pub fn dest_mode(&self) -> u32 {
        self.full.get_bits(2..3) as u32
    }
    /// rh: 3
    pub fn rh(&self) -> u32 {
        self.full.get_bits(3..4) as u32
    }
    /// rsvd_2: 4:11
    pub fn rsvd_2(&self) -> u32 {
        self.full.get_bits(4..12) as u32
    }
    /// dest_field: 12:19
    pub fn dest_field(&self) -> u32 {
        self.full.get_bits(12..20) as u32
    }
    /// addr_base: 20:31
    pub fn addr_base(&self) -> u32 {
        self.full.get_bits(20..32) as u32
    }
    /// hi_32: 32:63
    pub fn hi_32(&self) -> u32 {
        self.full.get_bits(32..64) as u32
    }
    /// intr_index_high: 2
    pub fn intr_index_high(&self) -> u32 {
        self.full.get_bits(2..3) as u32
    }
    /// shv: 3
    pub fn shv(&self) -> u32 {
        self.full.get_bits(3..4) as u32
    }
    /// intr_format: 4
    pub fn intr_format(&self) -> u32 {
        self.full.get_bits(4..5) as u32
    }
    /// intr_index_low: 5:19
    pub fn intr_index_low(&self) -> u32 {
        self.full.get_bits(5..20) as u32
    }
    /// constant: 20:31
    pub fn constant(&self) -> u32 {
        self.full.get_bits(20..32) as u32
    }
}

#[repr(C)]
pub struct MsiDataReg {
    full: u32,
}

impl From<u32> for MsiDataReg {
    fn from(item: u32) -> Self {
        MsiDataReg { full: item }
    }
}

impl From<MsiDataReg> for u32 {
    fn from(item: MsiDataReg) -> Self {
        item.full
    }
}

impl MsiDataReg {
    /// vector: 0:7
    pub fn vector(&self) -> u8 {
        self.full.get_bits(0..8) as u8
    }
    /// delivery_mode: 8:10
    pub fn delivery_mode(&self) -> u8 {
        self.full.get_bits(8..11) as u8
    }
    /// rsvd_1: 11:14
    pub fn rsvd_1(&self) -> u8 {
        self.full.get_bits(11..14) as u8
    }
    /// level: 14
    pub fn level(&self) -> bool {
        self.full.get_bit(14)
    }
    /// trigger_mode: 15
    pub fn trigger_mode(&self) -> bool {
        self.full.get_bit(15)
    }
    /// rsvd_2: 16:31
    pub fn rsvd_2(&self) -> u16 {
        self.full.get_bits(16..32) as u16
    }
}

#[derive(Copy, Clone, Default, Debug)]
pub struct MsiVector {
    pub msi_addr: u64,
    // [0:31]: data, [32:63]: vector control
    pub msi_data: u64,
}
/// MSI-X message structure.
#[derive(Copy, Clone)]
pub struct Message {
    /// Lower 32bit address of MSI-X address.
    pub address: u64,
    /// MSI-X data.
    pub data: u32,
}

/// MSI-X structure.
pub struct Msix {
    /// MSI-X table.
    pub table: Vec<u8>,
    pba: Vec<u8>,
    pub func_masked: bool,
    pub enabled: bool,
    pub msix_cap_offset: u16,
    pub dev_id: Arc<AtomicU16>,
    pub msi_irq_manager: Option<Arc<dyn MsiIrqManager>>,
}
impl Msix {
    /// Construct a new MSI-X structure.
    ///
    /// # Arguments
    ///
    /// * `table_size` - Size in bytes of MSI-X table.
    /// * `pba_size` - Size in bytes of MSI-X PBA.
    /// * `msix_cap_offset` - Offset of MSI-X capability in configuration space.
    /// * `dev_id` - Dev_id for device.
    pub fn new(
        table_size: u32,
        pba_size: u32,
        msix_cap_offset: u16,
        dev_id: Arc<AtomicU16>,
        msi_irq_manager: Option<Arc<dyn MsiIrqManager>>,
    ) -> Self {
        let mut msix = Msix {
            table: vec![0; table_size as usize],
            pba: vec![0; pba_size as usize],
            func_masked: true,
            enabled: true,
            msix_cap_offset,
            dev_id,
            msi_irq_manager,
        };
        msix.mask_all_vectors();
        msix
    }

    pub fn reset(&mut self) {
        self.table.fill(0);
        self.pba.fill(0);
        self.func_masked = true;
        self.enabled = true;
        self.mask_all_vectors();
    }

    pub fn is_enabled(&self, config: &[u8]) -> bool {
        let offset: usize = self.msix_cap_offset as usize + MSIX_CAP_CONTROL as usize;
        let msix_ctl = le_read_u16(config, offset).unwrap();
        if msix_ctl & MSIX_CAP_ENABLE > 0 {
            return true;
        }
        false
    }

    pub fn is_func_masked(&self, config: &[u8]) -> bool {
        let offset: usize = self.msix_cap_offset as usize + MSIX_CAP_CONTROL as usize;
        let msix_ctl = le_read_u16(config, offset).unwrap();
        if msix_ctl & MSIX_CAP_FUNC_MASK > 0 {
            return true;
        }
        false
    }

    fn mask_all_vectors(&mut self) {
        let nr_vectors: usize = self.table.len() / MSIX_TABLE_ENTRY_SIZE as usize;
        for v in 0..nr_vectors {
            let offset: usize = v * MSIX_TABLE_ENTRY_SIZE as usize + MSIX_TABLE_VEC_CTL as usize;
            self.table[offset] |= MSIX_TABLE_MASK_BIT;
        }
    }

    pub fn is_vector_masked(&self, vector: u16) -> bool {
        if !self.enabled || self.func_masked {
            return true;
        }

        let offset = (vector * MSIX_TABLE_ENTRY_SIZE + MSIX_TABLE_VEC_CTL) as usize;
        if self.table[offset] & MSIX_TABLE_MASK_BIT == 0 {
            return false;
        }
        true
    }

    fn is_vector_pending(&self, vector: u16) -> bool {
        let offset: usize = vector as usize / 64;
        let pending_bit: u64 = 1 << (vector as u64 % 64);
        let value = le_read_u64(&self.pba, offset).unwrap();
        if value & pending_bit > 0 {
            return true;
        }
        false
    }

    fn set_pending_vector(&mut self, vector: u16) {
        let offset: usize = vector as usize / 64;
        let pending_bit: u64 = 1 << (vector as u64 % 64);
        let old_val = le_read_u64(&self.pba, offset).unwrap();
        le_write_u64(&mut self.pba, offset, old_val | pending_bit).unwrap();
    }

    fn clear_pending_vector(&mut self, vector: u16) {
        let offset: usize = vector as usize / 64;
        let pending_bit: u64 = !(1 << (vector as u64 % 64));
        let old_val = le_read_u64(&self.pba, offset).unwrap();
        le_write_u64(&mut self.pba, offset, old_val & pending_bit).unwrap();
    }

    pub fn clear_pending_vectors(&mut self) {
        let max_vector_nr = self.table.len() as u16 / MSIX_TABLE_ENTRY_SIZE;
        for v in 0..max_vector_nr {
            self.clear_pending_vector(v);
        }
    }

    pub fn get_msix_vector(&self, vector: u16) -> MsiVector {
        let entry_offset: u16 = vector * MSIX_TABLE_ENTRY_SIZE;
        let mut offset = entry_offset as usize;
        let address = le_read_u64(&self.table, offset).unwrap();
        offset = (entry_offset + MSIX_MSG_DATA) as usize;
        let data = le_read_u64(&self.table, offset).unwrap();

        MsiVector {
            msi_addr: address,
            msi_data: data,
        }
    }

    pub fn send_msix(&self, vector: u16, dev_id: u16) {
        let msix_vector = self.get_msix_vector(vector);

        let irq_manager = self.msi_irq_manager.as_ref().unwrap();
        if let Err(e) = irq_manager.trigger(msix_vector, dev_id as u32) {
            error!("Send msix error: {:?}", e);
        };
    }

    pub fn notify(&mut self, vector: u16, dev_id: u16) {
        if vector >= self.table.len() as u16 / MSIX_TABLE_ENTRY_SIZE {
            warn!("Invalid msix vector {}.", vector);
            return;
        }

        if self.is_vector_masked(vector) {
            self.set_pending_vector(vector);
            return;
        }

        self.send_msix(vector, dev_id);
    }

    pub fn write_config(&mut self, config: &[u8], dev_id: u16, offset: usize, data: &[u8]) {
        let len = data.len();
        let msix_cap_control_off: usize = self.msix_cap_offset as usize + MSIX_CAP_CONTROL as usize;
        // Only care about the bits Masked(14) & Enabled(15) in msix control register.
        // SAFETY: msix_cap_control_off is less than u16::MAX.
        // Offset and len have been checked in call function PciConfig::write.
        if !ranges_overlap(offset, len, msix_cap_control_off + 1, 1).unwrap() {
            return;
        }

        let masked: bool = self.is_func_masked(config);
        let enabled: bool = self.is_enabled(config);

        let mask_state_changed = !((self.func_masked == masked) && (self.enabled == enabled));

        self.func_masked = masked;
        self.enabled = enabled;

        if mask_state_changed && (self.enabled && !self.func_masked) {
            let max_vectors_nr: u16 = self.table.len() as u16 / MSIX_TABLE_ENTRY_SIZE;
            for v in 0..max_vectors_nr {
                if !self.is_vector_masked(v) && self.is_vector_pending(v) {
                    self.clear_pending_vector(v);
                    self.send_msix(v, dev_id);
                }
            }
        }
    }

    fn generate_region_ops(
        msix: Arc<Mutex<Self>>,
        dev_id: Arc<AtomicU16>,
    ) -> HyperResult<RegionOps> {
        // let locked_msix = msix.lock();
        // let table_size = locked_msix.table.len() as u64;
        // let pba_size = locked_msix.pba.len() as u64;

        let cloned_msix = msix.clone();
        let read = move |offset: u64, access_size: u8| -> HyperResult<u64> {
            let mut data = [0u8; 8];
            let access_offset = offset as usize + access_size as usize;
            if access_offset > cloned_msix.lock().table.len() {
                if access_offset > cloned_msix.lock().table.len() + cloned_msix.lock().pba.len() {
                    error!(
                        "Fail to read msix table and pba, illegal data length {}, offset {}",
                        access_size, offset
                    );
                    return Err(HyperError::OutOfRange);
                }
                // deal with pba read
                let offset = offset as usize;
                data.copy_from_slice(
                    &cloned_msix.lock().pba[offset..(offset + access_size as usize)],
                );
                return Ok(u64::from_le_bytes(data));
            }
            // msix table read
            data.copy_from_slice(
                &cloned_msix.lock().table
                    [offset as usize..(offset as usize + access_size as usize)],
            );
            Ok(u64::from_le_bytes(data))
        };

        let cloned_msix = msix.clone();
        let write = move |offset: u64, access_size: u8, data: &[u8]| -> HyperResult {
            let access_offset = offset as usize + access_size as usize;
            if access_offset > cloned_msix.lock().table.len() {
                if access_offset > cloned_msix.lock().table.len() + cloned_msix.lock().pba.len() {
                    error!(
                        "It's forbidden to write out of the msix table and pba (size: {}), with offset of {} and size of {}",
                        cloned_msix.lock().table.len(),
                        offset,
                        data.len()
                    );
                    return Err(HyperError::OutOfRange);
                }
                // deal with pba read
                return Ok(());
            }
            let mut locked_msix = cloned_msix.lock();
            let vector: u16 = offset as u16 / MSIX_TABLE_ENTRY_SIZE;
            let was_masked: bool = locked_msix.is_vector_masked(vector);
            let offset = offset as usize;
            locked_msix.table[offset..(offset + 4)].copy_from_slice(data);

            let is_masked: bool = locked_msix.is_vector_masked(vector);

            // Clear the pending vector just when it is pending. Otherwise, it
            // will cause unknown error.
            if was_masked && !is_masked && locked_msix.is_vector_pending(vector) {
                locked_msix.clear_pending_vector(vector);
                locked_msix.notify(vector, dev_id.load(Ordering::Acquire));
            }

            Ok(())
        };
        let msix_region_ops = RegionOps {
            read: Arc::new(read),
            write: Arc::new(write),
        };

        Ok(msix_region_ops)
    }
}

/// MSI-X initialization.
///
/// # Arguments
///
/// * `pcidev_base ` - The Base of PCI device
/// * `bar_id` - BAR id.
/// * `vector_nr` - The number of vector.
/// * `dev_id` - Dev id.
/// * `parent_region` - Parent region which the MSI-X region registered. If none, registered in BAR.
/// * `offset_opt` - Offset of table(table_offset) and Offset of pba(pba_offset). Set the
///   table_offset and pba_offset together.
pub fn init_msix<B: BarAllocTrait>(
    pcidev_base: &mut PciDevBase<B>,
    bar_id: usize,
    vector_nr: u32,
    dev_id: Arc<AtomicU16>,
    // parent_region: Option<&Region>,
    offset_opt: Option<(u32, u32)>,
) -> HyperResult<()> {
    let config = &mut pcidev_base.config;
    let parent_bus = &pcidev_base.parent_bus;
    if vector_nr == 0 || vector_nr > MSIX_TABLE_SIZE_MAX as u32 + 1 {
        error!(
            "invalid msix vectors, which should be in [1, {}]",
            MSIX_TABLE_SIZE_MAX + 1
        );
    }

    let msix_cap_offset: usize = config.add_pci_cap(CapId::Msix as u8, MSIX_CAP_SIZE as usize)?;
    let mut offset: usize = msix_cap_offset + MSIX_CAP_CONTROL as usize;
    le_write_u16(&mut config.config, offset, vector_nr as u16 - 1)?;
    le_write_u16(
        &mut config.write_mask,
        offset,
        MSIX_CAP_FUNC_MASK | MSIX_CAP_ENABLE,
    )?;
    offset = msix_cap_offset + MSIX_CAP_TABLE as usize;
    let table_size = vector_nr * MSIX_TABLE_ENTRY_SIZE as u32;
    let pba_size = ((round_up(vector_nr as u64, 64).unwrap() / 64) * 8) as u32;
    let (table_offset, pba_offset) = offset_opt.unwrap_or((0, table_size));
    if ranges_overlap(
        table_offset as usize,
        table_size as usize,
        pba_offset as usize,
        pba_size as usize,
    )
    .unwrap()
    {
        error!("msix table and pba table overlapped.");
    }
    le_write_u32(&mut config.config, offset, table_offset | bar_id as u32)?;
    offset = msix_cap_offset + MSIX_CAP_PBA as usize;
    le_write_u32(&mut config.config, offset, pba_offset | bar_id as u32)?;

    let msi_irq_manager = if let Some(pci_bus) = parent_bus.upgrade() {
        let locked_pci_bus = pci_bus.lock();
        locked_pci_bus.get_msi_irq_manager()
    } else {
        error!("Msi irq controller is none");
        None
    };

    let msix = Arc::new(Mutex::new(Msix::new(
        table_size,
        pba_size,
        msix_cap_offset as u16,
        dev_id.clone(),
        msi_irq_manager,
    )));
    let mut bar_size = ((table_size + pba_size) as u64).next_power_of_two();
    bar_size = max(bar_size, MINIMUM_BAR_SIZE_FOR_MMIO as u64);
    let msix_region_ops = Msix::generate_region_ops(msix.clone(), dev_id).unwrap();
    config.register_bar(
        bar_id,
        Some(msix_region_ops),
        RegionType::Mem32Bit,
        false,
        bar_size,
    )?;

    config.msix = Some(msix.clone());

    Ok(())
}
