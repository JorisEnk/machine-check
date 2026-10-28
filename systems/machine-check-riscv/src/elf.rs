use std::collections::BTreeMap;

use machine_check::{Bitvector, BitvectorArray};
use object::{
    read::elf::{ElfFile32, ProgramHeader},
    LittleEndian,
};

use super::dwarf;

use crate::{
    dwarf::{Symbol, Symbols},
    system::machine_module::System,
};

// defines
const SRAM_STATIC_BASE: u32 = 0x2000_0000;
const SRAM_DYNAMIC_BASE: u32 = 0x2000_4000;
const PC_ENTRY: u32 = 0x2000_1000;

/// Parses an ELF file and returns the system and usable symbols.
pub fn parse_elf(path: &str) -> anyhow::Result<(System, dwarf::Symbols)> {
    // zero is guaranteed-illegal instruction
    let byte_zero = Bitvector::<8>::new(0);

    // 0x4000 -> 4*16^3 = 2^2 * 2^12 = 2^14 -> 14 bits or short 0x3FFF -> 11 1111 1111 1111 -> 14
    let mut sram_static = BitvectorArray::<14, 8>::new_filled(byte_zero);
    let mut sram_dynamic = BitvectorArray::<14, 8>::new_filled(byte_zero);
    let pc_entry = Bitvector::new(PC_ENTRY as u64);

    // load the program flash from program headers
    let elf_bytes = std::fs::read(path)?;
    let elf_file = ElfFile32::<LittleEndian>::parse(&*elf_bytes)?;

    for program_header in elf_file.elf_program_headers() {
        let header_type = program_header.p_type.get(LittleEndian);

        if header_type != 0x01 {
            // not LOAD
            continue;
        }

        let address = program_header.p_paddr.get(LittleEndian);
        let size = program_header.p_memsz.get(LittleEndian);

        let data = program_header
            .data(LittleEndian, &*elf_bytes)
            .expect("Program header data should be present");

        // To Do addressspace checking if it fits
        match address {
            0x2000_0000..0x2000_4000 => {
                let mut sram_static_addr: u32 = address - SRAM_STATIC_BASE;
                for value in data {
                    sram_static[Bitvector::new(sram_static_addr as u64)] =
                        Bitvector::<8>::new(*value as u64);
                    // Index of Bitvector +1 because width is 8 bits one byte as vlaue
                    sram_static_addr += 1;
                }
            }
            0x2000_4000..0x2000_8000 => {
                let mut sram_dynamic_addr: u32 = address - SRAM_DYNAMIC_BASE;
                for value in data {
                    sram_dynamic[Bitvector::new(sram_dynamic_addr as u64)] =
                        Bitvector::<8>::new(*value as u64);
                    sram_dynamic_addr += 1;
                }
            }
            _ => {
                unimplemented!(
                    "ELF program header with offset {:x}, size {:x}",
                    address,
                    size
                )
            }
        }
    }

    // load the symbols
    let symbols = Symbols::from_elf_file(&elf_file)?;

    if log::log_enabled!(log::Level::Debug) {
        // debug-log the symbols
        // sort alphabetically and filter out unusable symbols
        let mut usable = BTreeMap::new();

        for (name, symbol) in symbols.inner().iter() {
            if !matches!(symbol, Symbol::Unresolved | Symbol::Multiple) {
                usable.insert(name, symbol);
            }
        }

        log::debug!("Usable symbols: {:#X?}", usable);
    }
    // also
    let system = System {
        sram_static,
        sram_dynamic,
        pc_entry,
    };

    Ok((system, symbols))
}
