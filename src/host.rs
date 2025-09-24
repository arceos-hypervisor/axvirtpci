use core::ops::Range;

use axerrno::{AxError, AxResult};

#[cfg(target_arch = "x86_64")]
use crate::{le_read_u32, le_write_u32};

use crate::PioOps;

const PCI_CFG_ADDR_PORT: Range<u16> = 0xcf8..0xcf8 + 4;
const PCI_CFG_DATA_PORT: Range<u16> = 0xcfc..0xcfc + 4;

#[derive(Clone)]
pub struct PciHost {
    // pub root_bus: Arc<Mutex<PciBus<B>>>,
    #[cfg(target_arch = "x86_64")]
    config_addr: u32,
    check_type1: usize,
}

impl PciHost {
    /// Construct PCI/PCIe host.
    pub fn new() -> Self {
        PciHost {
            // root_bus: Arc::new(Mutex::new(root_bus)),
            config_addr: 0,
            check_type1: 0,
        }
    }
}

impl PioOps for PciHost {
    fn port_range(&self) -> Range<u16> {
        PCI_CFG_ADDR_PORT.start..PCI_CFG_DATA_PORT.end
    }

    fn read(&mut self, port: u16, access_size: u8) -> AxResult<u32> {
        let mut data = [0xffu8; 4]; // max access size is 4
        let cloned_hb = self.clone();
        if PCI_CFG_ADDR_PORT.contains(&port) {
            // Read configuration address register.
            if port == 0xcf8 && self.check_type1 == 2 {
                self.check_type1 = 0;
                return Ok(0x8000_0000);
            } else {
                // also deal with tmp = inl(0xCF8); in check type
                le_write_u32(&mut data[..], 0, cloned_hb.config_addr).unwrap();
            }
        } else {
            // Read configuration data register.
            // if access_size > 4 || cloned_hb.config_addr & CONFIG_ADDRESS_ENABLE_MASK == 0 {
            //     return Err(AxError::InvalidInput);
            // }
            // let mut offset: u32 = (cloned_hb.config_addr & !CONFIG_ADDRESS_ENABLE_MASK)
            //     + (port - PCI_CFG_DATA_PORT.start) as u32;
            // // debug!("in pci read: offset:{:#x}", offset);
            // let bus_num = ((offset >> PIO_BUS_SHIFT) & CONFIG_BUS_MASK) as u8;
            // let devfn = ((offset >> PIO_DEVFN_SHIFT) & CONFIG_DEVFN_MASK) as u8;
            // match cloned_hb.find_device(bus_num, devfn) {
            //     Some(dev) => {
            //         offset &= PIO_OFFSET_MASK;
            //         dev.lock().read_config(offset as usize, &mut data[..]);
            //     }
            //     None => {
            for d in data.iter_mut() {
                *d = 0xff;
            }
            // }
            // }
        }
        match access_size {
            1 => Ok(u32::from_le_bytes([data[0], 0, 0, 0])),
            2 => Ok(u32::from_le_bytes([data[0], data[1], 0, 0])),
            4 => Ok(u32::from_le_bytes(data)),
            _ => Err(AxError::InvalidInput),
        }
    }

    fn write(&mut self, port: u16, access_size: u8, value: u32) -> AxResult {
        // debug!(
        //     "this is pci host write port:{:#x} access_size:{} value:{:#x}",
        //     port, access_size, value
        // );
        if PCI_CFG_ADDR_PORT.contains(&port) {
            // Write configuration address register.
            // deal with pci_check_type1 in linux
            if port == 0xcfb && access_size == 1 {
                self.check_type1 = 1;
                // do nothing for read from 0xcf8; 1: outb(0x01, 0xCFB); then will tmp = inl(0xCF8);
            } else {
                if self.check_type1 == 1 {
                    self.check_type1 = 2;
                } else {
                    // save bdf for next read/write
                    self.config_addr = le_read_u32(&value.to_le_bytes(), 0).unwrap();
                }
            }
        } else {
            // Write configuration data register.
            // if access_size > 4 || self.config_addr & CONFIG_ADDRESS_ENABLE_MASK == 0 {
            //     return Err(AxError::InvalidInput);
            // }

            // let mut offset: u32 = (self.config_addr & !CONFIG_ADDRESS_ENABLE_MASK)
            //     + (port - PCI_CFG_DATA_PORT.start) as u32;
            // let bus_num = ((offset >> PIO_BUS_SHIFT) & CONFIG_BUS_MASK) as u8;
            // let devfn = ((offset >> PIO_DEVFN_SHIFT) & CONFIG_DEVFN_MASK) as u8;

            // if let Some(dev) = self.find_device(bus_num, devfn) {
            //     offset &= PIO_OFFSET_MASK;
            //     let value_bytes = value.to_le_bytes();
            //     let value_byte: &[u8] = match access_size {
            //         1 => &value_bytes[0..1],
            //         2 => &value_bytes[0..2],
            //         4 => &value_bytes[0..4],
            //         _ => return Err(AxError::InvalidInput),
            //     };
            //     dev.lock().write_config(offset as usize, value_byte);
            // }
        }
        Ok(())
    }
}
