use std::io;
use std::ops::{DerefMut, Range};
use std::sync::Mutex;

use bytemuck::{bytes_of, bytes_of_mut};

use read_write_at::ReadAtMut;




bitflags::bitflags! {
    #[derive(Default)]
    #[repr(transparent)]
    pub struct MemoryFlags : u32 {
        const NONE      = 0;
        const READ      = 0x00000001;
        const WRITE     = 0x00000002;
        const EXECUTE   = 0x00000004;
    }
}

pub struct Memory {
    pub pages: Vec<Mutex<Page>>, // 1<<22 entries is 4M * size_of::<Page>(), too big to fit on stack
}

pub struct Page {
    pub data:   Option<Box<[u64; 4096/8]>>,
    pub flags:  MemoryFlags,
}

impl Default for Memory {
    fn default() -> Self {
        let mut pages = Vec::new();
        pages.reserve_exact(1 << 22);
        for _ in 0 .. 1 << 22 { pages.push(Mutex::new(Page::new())); }
        Self { pages }
    }
}

impl Default for Page {
    fn default() -> Self {
        Self {
            data:   None,
            flags:  MemoryFlags::NONE,
        }
    }
}

impl Memory {
    pub fn new() -> Self { Default::default() }

    pub fn init_copy_io(&mut self, base: u32, flags: MemoryFlags, io: &mut impl ReadAtMut, mut offset: u64, io_bytes: u32) -> io::Result<()> {
        self.init_pages(base, flags, io_bytes, |page, range| {
            let data = page.alloc_bytes_mut();
            io.read_exact_at(&mut data[range.start as usize .. range.end as usize], offset)?;
            offset += range.len() as u64;
            Ok(())
        })
    }

    pub fn init_zero(&mut self, base: u32, flags: MemoryFlags, zero_bytes: u32) -> io::Result<()> {
        self.init_pages(base, flags, zero_bytes, |_page, _bytes| {
            //let _ = page.alloc_bytes_mut();
            Ok(())
        })
    }

    pub fn read_u8(&self, addr: u32, flags: MemoryFlags) -> u8 { let mut result = 0u8; self.read_aligned(addr, flags, bytes_of_mut(&mut result)); result }
    pub fn read_u16_aligned(&self, addr: u32, flags: MemoryFlags) -> u16 { let mut result = 0u16; self.read_aligned(addr, flags, bytes_of_mut(&mut result)); u16::from_le(result) }
    pub fn read_u32_aligned(&self, addr: u32, flags: MemoryFlags) -> u32 { let mut result = 0u32; self.read_aligned(addr, flags, bytes_of_mut(&mut result)); u32::from_le(result) }
    pub fn read_u64_aligned(&self, addr: u32, flags: MemoryFlags) -> u64 { let mut result = 0u64; self.read_aligned(addr, flags, bytes_of_mut(&mut result)); u64::from_le(result) }
    pub fn read_u16_unaligned(&self, addr: u32, flags: MemoryFlags) -> u16 { let mut result = 0u16; self.read_unaligned(addr, flags, bytes_of_mut(&mut result)); u16::from_le(result) }
    pub fn read_u32_unaligned(&self, addr: u32, flags: MemoryFlags) -> u32 { let mut result = 0u32; self.read_unaligned(addr, flags, bytes_of_mut(&mut result)); u32::from_le(result) }
    pub fn read_u64_unaligned(&self, addr: u32, flags: MemoryFlags) -> u64 { let mut result = 0u64; self.read_unaligned(addr, flags, bytes_of_mut(&mut result)); u64::from_le(result) }

    pub fn read_bytes(&self, addr: u32, flags: MemoryFlags, bytes: &mut [u8]) { self.read_unaligned(addr, flags, bytes) }

    fn read_aligned(&self, addr: u32, flags: MemoryFlags, bytes: &mut [u8]) {
        let page_idx = usize::try_from(addr >> 10).unwrap();
        let offset = (addr & 0x3FF) as usize;
        let page = self.pages[page_idx].lock().unwrap();
        assert!(page.flags.contains(flags), "arm::Memory::read_aligned: page 0x{:08x} not mapped for read", page_idx << 10);
        bytes.copy_from_slice(&page.bytes()[offset..][..bytes.len()]);
    }

    fn read_unaligned(&self, addr: u32, flags: MemoryFlags, mut bytes: &mut [u8]) {
        let mut page_idx = usize::try_from(addr >> 10).unwrap();
        let mut offset = (addr & 0x3FF) as usize;

        while !bytes.is_empty() {
            let page_remaining = 0x400 - offset;
            let read = page_remaining.min(bytes.len());
            let page = self.pages[page_idx].lock().unwrap();
            assert!(page.flags.contains(flags), "arm::Memory::read_unaligned: page 0x{:08x} not mapped for read", page_idx << 10);
            bytes[..read].copy_from_slice(&page.bytes()[offset..][..read]);

            bytes = &mut bytes[read..];
            page_idx += 1;
            offset = 0;
        }
    }
}

impl Memory {
    fn init_pages(&mut self, base: u32, flags: MemoryFlags, mut bytes: u32, mut on_page: impl FnMut(&mut Page, Range<u32>) -> io::Result<()>) -> io::Result<()> {
        let mut page_idx = base >> 10;

        // special case first page
        if bytes > 0 {
            let offset = base & 0x3FF;
            let size = (0x400 - offset).min(bytes);
            let mut page = self.init_page(page_idx, flags)?;
            on_page(&mut *page, offset .. offset + size)?;
            page_idx += 1;
            bytes -= size;
        }

        while bytes > 0 {
            let size = bytes.min(0x400);
            let mut page = self.init_page(page_idx, flags)?;
            on_page(&mut *page, 0 .. size)?;
            page_idx += 1;
            bytes -= size;
        }

        Ok(())
    }

    fn init_page<'a>(&'a mut self, page_idx: u32, flags: MemoryFlags) -> io::Result<impl DerefMut<Target = Page> + 'a> {
        let page_idx = usize::try_from(page_idx).map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "arm::Memory: tried to initialize beyond address space"))?;
        let page = self.pages.get(page_idx).ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "arm::Memory: tried to initialize beyond address space"))?;
        let mut page = page.lock().unwrap(); // panic on poisoned lock
        page.flags |= flags;
        Ok(page)
    }
}

impl Page {
    pub fn new() -> Self { Default::default() }

    pub fn alloc_bytes_mut(&mut self) -> &mut [u8] {
        bytes_of_mut(&mut **self.data.get_or_insert_with(|| Box::new([0u64; 512])))
    }

    pub fn bytes(&self) -> &[u8] {
        bytes_of(self.data.as_ref().map_or(&ZEROS, |data| &**data))
    }
}

const ZEROS : [u64; 512] = [0; 512];
