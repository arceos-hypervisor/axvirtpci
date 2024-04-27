use alloc::string::String;
use alloc::sync::Arc;
use bit_field::BitField;
use core::ops::Range;
use spin::Mutex;

use crate::{bus::PciBus, MsiIrqManager, PciDevOps};

use hypercraft::PioOps;
use hypercraft::{HyperError, HyperResult};

const CONFIGURATION_SPACE_ADDRESS_PORT_OFFSET: usize = 0;
const CONFIGURATION_SPACE_ADDRESS_PORT_LAST_OFFSET: usize = 3;
const CONFIGURATION_SPACE_DATA_PORT_OFFSET: usize = 4;
const CONFIGURATION_SPACE_DATA_PORT_LAST_OFFSET: usize = 7;

const PCI_CFG_ADDR_PORT: u16 = 0xcf8;

#[derive(Clone)]
pub struct DummyPciHost {
    port_base: u16,
    current_address: u64,
    pub root_bus: Arc<Mutex<PciBus>>,
}

impl DummyPciHost {
    /// Construct PCI/PCIe host.
    pub fn new(msi_irq_manager: Option<Arc<dyn MsiIrqManager>>) -> Self {
        let root_bus = PciBus::new(String::from("pcie.0"), msi_irq_manager);
        Self {
            root_bus: Arc::new(Mutex::new(root_bus)),
            port_base: PCI_CFG_ADDR_PORT,
            current_address: 0,
        }
    }

    pub fn find_device(&self, bus_num: u8, devfn: u8) -> Option<Arc<Mutex<dyn PciDevOps>>> {
        None
    }
}

impl PioOps for DummyPciHost {
    fn port_range(&self) -> core::ops::Range<u16> {
        return self.port_base..self.port_base + 8;
    }

    fn read(&mut self, port: u16, access_size: u8) -> HyperResult<u32> {
        match (port - self.port_base) as usize {
            _offset @ CONFIGURATION_SPACE_ADDRESS_PORT_OFFSET
                ..=CONFIGURATION_SPACE_ADDRESS_PORT_LAST_OFFSET => {
                // we return non-sense to tell linux pci is not present.
                match access_size {
                    1 => Ok(0xfe),
                    2 => Ok(0xfffe),
                    4 => Ok(0xffff_fffe),
                    _ => Err(HyperError::InvalidParam),
                }
            }
            CONFIGURATION_SPACE_DATA_PORT_OFFSET..=CONFIGURATION_SPACE_DATA_PORT_LAST_OFFSET => {
                match access_size {
                    1 => Ok(0xff),
                    2 => Ok(0xffff),
                    4 => Ok(0xffff_ffff),
                    _ => Err(HyperError::InvalidParam),
                }
            }
            _ => Err(HyperError::InvalidParam),
        }
    }

    fn write(&mut self, port: u16, access_size: u8, value: u32) -> HyperResult {
        match (port - self.port_base) as usize {
            offset @ CONFIGURATION_SPACE_ADDRESS_PORT_OFFSET
                ..=CONFIGURATION_SPACE_ADDRESS_PORT_LAST_OFFSET => match access_size {
                1 => Ok({
                    self.current_address
                        .set_bits(offset * 8..offset * 8 + 8, value as u8 as u64);
                }),
                2 => Ok({
                    self.current_address
                        .set_bits(offset * 8..offset * 8 + 16, value as u16 as u64);
                }),
                4 => Ok({
                    self.current_address
                        .set_bits(offset * 8..offset * 8 + 32, value as u64);
                }),
                _ => Err(HyperError::InvalidParam),
            },
            _ => Err(HyperError::NotSupported),
        }
    }
}
