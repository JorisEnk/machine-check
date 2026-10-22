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

/// Parses an ELF file and returns the system and usable symbols.
pub fn parse_elf(path: &str) -> anyhow::Result<(System, dwarf::Symbols)> {
    // zero is guaranteed-illegal instruction
    let byte_zero = Bitvector::<8>::new(0);

    // program flash
    // 0x0000_0000..0x0002_0000 (17 bits)
        // change index here to 20-1 "20 adress bits"
    // let mut sram_static = BitvectorArray::<18, 16>::new_filled(halfword_zero);
    let mut program_flash = BitvectorArray::<17, 8>::new_filled(byte_zero);

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

        match address {
            // to do chnage daress
            0x0000_0000..0x0002_0000 => {
                // load into program flash

                for value in data {
                    program_flash[Bitvector::new(address as u64)] =
                        Bitvector::<8>::new(*value as u64);
                }
            }
            0x0101_0008..0x0101_0034 => {
                // the option-setting memory cannot be read or written to
                // within the system, do not load
            }
            0x2000_0000..0x2000_1000 => {
                // ECC SRAM, not persistent, do not load
            }
            0x2000_4000..0x2000_7000 => {
                // parity SRAM, not persistent, do not load
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
    let system = System { sram_static, sram_dynamic };

    Ok((system, symbols))
}
