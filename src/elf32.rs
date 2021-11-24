use crate::arm;

use std::io;
use std::mem::size_of_val;

use bytemuck::{bytes_of_mut, Pod, Zeroable};
use read_write_at::ReadAtMut;



pub type Addr       = u32;
pub type Off        = u32;
pub type Section    = u16;
pub type Versym     = u16;

/// **E**lf **H**ea**d**e**r**
#[derive(Clone, Copy, Debug, Zeroable, Pod)] #[repr(C)] pub struct Ehdr {
    pub e_ident:        [u8; 16],
    pub e_type:         u16,
    pub e_machine:      u16,
    pub e_version:      u32,
    pub e_entry:        Addr,
    pub e_phoff:        Off,
    pub e_shoff:        Off,
    pub e_flags:        u32,
    pub e_ehsize:       u16,
    pub e_phentsize:    u16,
    pub e_phnum:        u16,
    pub e_shentsize:    u16,
    pub e_shnum:        u16,
    pub e_shstrndx:     u16,
}

// **P**rogram **H**ea**d**e**r**
#[derive(Clone, Copy, Debug, Zeroable, Pod)] #[repr(C)] pub struct Phdr {
    pub p_type:     u32,
    // p_flags goes here in 64-bit version
    pub p_offset:   Off,
    pub p_vaddr:    Addr,
    pub p_paddr:    Addr,
    pub p_filesz:   u32,
    pub p_memsz:    u32,
    pub p_flags:    u32,
    pub p_align:    u32,
}

pub fn run(elf: &mut impl ReadAtMut) -> io::Result<()> {
    macro_rules! invalid_data {
        ( $reason:expr ) => {
            return Err(io::Error::new(io::ErrorKind::InvalidData, concat!("uvm::elf::run: ", $reason)))
        };
    }

    let mut e_ident = [0u8; 16];
    elf.read_exact_at(&mut e_ident[..], 0)?;
    if e_ident[0..=3]   != *b"\x7FELF"  { invalid_data!("not an elf file (invalid magic)") } // EI_MAG0..=3
    if e_ident[4]       != 1            { invalid_data!("only 32-bit elfs are currently supported") } // EI_CLASS
    if e_ident[5]       != 1            { invalid_data!("only little-endian elfs are currently supported") } // EI_DATA
    if e_ident[6]       != 1            { invalid_data!("only EI_VERSION == 1 elfs are currently supported") } // EI_VERSION
    let _osabi = e_ident[7];
    let _abiversion = e_ident[8];
    let _padding = &e_ident[9..];

    let mut ehdr = Ehdr { e_ident, .. Zeroable::zeroed() };
    elf.read_exact_at(&mut bytes_of_mut(&mut ehdr)[16..], 16)?;
    if ehdr.e_type      != 2    { invalid_data!("only elf executables are currently supported (e_type != ET_EXEC)") }
    if ehdr.e_machine   != 40   { invalid_data!("only ARM elfs are currently supported (e_machine != EM_ARM)") }
    if ehdr.e_version   != 1    { invalid_data!("only e_version == 1 elfs are currently supported") }
    // e_entry
    if ehdr.e_phoff     == 0    { invalid_data!("executable elfs must have a program header table (e_phoff == 0)") }
    // e_shoff, e_flags
    if usize::from(ehdr.e_ehsize) < std::mem::size_of_val(&ehdr) { invalid_data!("e_ehsize < size_of::<Ehdr>()") }
    if ehdr.e_phentsize == 0    { invalid_data!("program header table entries must have nonzero size (e_phentsize == 0)") }
    if ehdr.e_phnum == 0        { invalid_data!("executables must have at least one entry in their program header table (e_phnum == 0)") }
    // e_shentsize, e_shnum, e_shstrndx

    let mut mem = arm::Memory::new();

    for iph in 0 .. ehdr.e_phnum {
        let mut phdr = Phdr::zeroed();
        let phdr_read = size_of_val(&phdr).min(ehdr.e_phentsize.into());
        let phdr_off = u64::from(ehdr.e_phoff) + u64::from(iph) * u64::from(ehdr.e_phentsize);
        elf.read_exact_at(&mut bytes_of_mut(&mut phdr)[..phdr_read], phdr_off)?;

        match phdr.p_type {
            0 => continue, // PT_NULL
            1 => { // PT_LOAD
                if phdr.p_filesz > phdr.p_memsz { invalid_data!("program segment file size (p_filesz) exceeds memory size (p_memsz)") }
                let io_size = phdr.p_filesz;
                let zero_size = phdr.p_memsz - phdr.p_filesz;

                let mut flags = arm::MemoryFlags::NONE;
                if phdr.p_flags & 0x1 != 0 { flags |= arm::MemoryFlags::EXECUTE; } // PF_X
                if phdr.p_flags & 0x2 != 0 { flags |= arm::MemoryFlags::WRITE;   } // PF_W
                if phdr.p_flags & 0x4 != 0 { flags |= arm::MemoryFlags::READ;    } // PF_R

                mem.init_zero(phdr.p_vaddr, flags, zero_size)?;
                mem.init_copy_io(phdr.p_vaddr, flags, elf, phdr.p_offset.into(), io_size)?;
            },
            2 => { // PT_DYNAMIC
                invalid_data!("phdr.p_type == PT_DYNAMIC not yet supported")
            },
            // ...
            _ => continue,
        }

    }

    let mem = mem; // !mut
    let mut core = arm::Cpu::new();
    core.set_next_instruction_addr(ehdr.e_entry);
    loop {
        core.step1(&mem);
    }
}
