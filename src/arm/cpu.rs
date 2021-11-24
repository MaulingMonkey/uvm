#![allow(unused_parens)]

use super::*;

// References:
// http://imrannazar.com/arm-opcode-map
// ARMv4? https://iitd-plos.github.io/col718/ref/arm-instructionset.pdf
// ARMv4? https://developer.arm.com/documentation/ddi0210/c/Introduction/Instruction-set-summary/Format-summary
// ARMv7? https://developer.arm.com/documentation/ddi0406/cb/Application-Level-Architecture/ARM-Instruction-Set-Encoding/ARM-instruction-set-encoding



#[derive(Clone, Debug, Default)]
pub struct Cpu {
    // https://developer.arm.com/documentation/dui0473/c/overview-of-the-arm-architecture/arm-registers

    pub registers: [u32; 16], // r0 ..= r12, sp/r13, lr/r14, pc/r15
    // flags: bit pack? per <https://developer.arm.com/documentation/ddi0210/c/Programmer-s-Model/The-program-status-registers>?
    pub z: bool,
    pub c: bool,
    pub n: bool,
    pub v: bool,
    // TODO: APSR?
    // TODO: privileged registers?
}

impl Cpu {
    pub fn new() -> Self { Default::default() }

    // https://developer.arm.com/documentation/ddi0406/cb/Application-Level-Architecture/Application-Level-Programmers--Model/ARM-core-registers?lang=en
    // Assume ARM mode for now
    fn read_pc_offset(&self) -> u32 { 8 }

    pub fn set_next_instruction_addr(&mut self, addr: u32) {
        self.registers[15] = addr + self.read_pc_offset();
    }

    pub fn step1(&mut self, mem: &Memory) {
        let op = mem.read_u32_aligned(self.registers[15] - self.read_pc_offset(), MemoryFlags::READ | MemoryFlags::EXECUTE);

        let cond = match op >> 28 {
            0b0000 => self.z,                           // EQ equal
            0b0001 => !self.z,                          // NE not equal
            0b0010 => self.c,                           // CS unsigned higher-or-same
            0b0011 => !self.c,                          // CC unsigned lower
            0b0100 => self.n,                           // MI (minus?) negative
            0b0101 => !self.n,                          // PL positive or zero
            0b0110 => self.v,                           // VS overflow
            0b0111 => !self.v,                          // VC no overflow
            0b1000 => self.c && !self.z,                // HI unsigned higher
            0b1001 => !self.c || self.z,                // LS unsigned lower or same
            0b1010 => self.n == self.v,                 // GE greater or equal
            0b1011 => self.n != self.v,                 // LT less than
            0b1100 => !self.z && (self.n == self.v),    // GT greater than
            0b1101 => self.z || (self.n != self.v),     // LE less than or equal
            0b1110 => true,                             // AL always
            _b1111 => {                                 // Unconditional opcode
                false // don't do the traditional cond op
            },
        };

        if cond {
            // Ref: 4.1.1 Format summary
            // Is it just me, or are there a lot of potentially overlapping encodings in said table?

            if (op >> 4) & 0xFFFFFF == 0b0001_0010_1111_1111_1111_0001 {
                panic!("arm::Cpu::step1: BX not yet implemented");
            // } else if (op >> 4) & 0b1111 == 0b1001 {
            //     // ...
            } else {
                match (op >> 20) & 0xFF {
                    0x28 => self.impl_data_processing(op), // ADD
                    0x3A => self.impl_data_processing(op), // MOV

                    // 0x00 => panic!("and?"), // AND / MUL
                    // 0x3B => panic!("movs"),

                    0xF0 ..= 0xFF => self.impl_swi(mem, op),
                    _other => panic!("arm::Cpu::step1: unimplemented op: 0x{:08x} / 0b{:032b}", op, op),
                }
            }
        }

        self.registers[15] += 4;
    }

    // 4.3 Branch and Exchange (BX)
    // 4.4 Branch and Branch with Link (B, BL)
    // TODO: implement

    /// 4.5 Data Processing
    fn impl_data_processing(&mut self, op: u32) {
        let _cond       = ((op >> 28) & 0b1111);
        let _sel1       = ((op >> 26) & 0b11);
        let immediate   = ((op >> 25) & 0b1) == 1;
        let opcode      = ((op >> 21) & 0b1111);
        let _setcc      = ((op >> 20) & 0b1) == 1;
        let rn          = ((op >> 16) & 0b1111) as usize; // ignored by mov
        let op1         = self.registers[rn];
        let rd          = ((op >> 12) & 0b1111) as usize;
        let op2         = match immediate {
            false => {
                let rm              = (op >> 0) & 0xF;
                let rm              = self.registers[rm as usize];
                let shift_type      = (op >> 5) & 0x3;
                let shift_amount    = match ((op >> 4) & 0b1) == 1 {
                    false => (op >> 7) & 0x1F,
                    true => {
                        let rs      = ((op >> 8) & 0xF) as usize;
                        self.registers[rs] & 0x1F // "The amount by which the register should be shifted may be [...] in the bottom byte of another register (other than R15)." (4.5.2)
                    },
                };
                match shift_type {
                    0b00 => rm.wrapping_shl(shift_amount),                  // logical left
                    0b01 => rm.wrapping_shr(shift_amount),                  // logical right
                    0b10 => (rm as i32).wrapping_shr(shift_amount) as u32,  // arithmetic right
                    _b11 => rm.rotate_right(shift_amount),                  // rotate right
                }
            },
            true => {
                let rotate  = ((op >> 8) & 0b1111);
                let imm     = ((op >> 0) & 0b1111_1111);
                imm.rotate_right(2 * rotate) // 4.5.3 Immediate operand rotates
            },
        };

        debug_assert_eq!(_sel1, 0b00);
        assert_eq!(_setcc, false, "setcc not yet implemented");

        match opcode {
            0b0000 => self.registers[rd] = op1 & op2, // AND
            0b0001 => self.registers[rd] = op1 ^ op2, // EOR
            0b0010 => self.registers[rd] = op1.wrapping_sub(op2), // SUB
            0b0011 => self.registers[rd] = op2.wrapping_sub(op1), // RSB
            0b0100 => self.registers[rd] = op1.wrapping_add(op2), // ADD
            0b0101 => self.registers[rd] = op1.wrapping_add(op2).wrapping_add(self.c as u32), // ADC
            0b0110 => self.registers[rd] = op1.wrapping_sub(op2).wrapping_add(self.c as u32).wrapping_sub(1), // SBC
            0b0111 => self.registers[rd] = op2.wrapping_sub(op1).wrapping_add(self.c as u32).wrapping_sub(1), // RSC
            0b1000 => panic!("tst not yet implemented"), // and, but result is not written
            0b1001 => panic!("teq not yet implemented"), // eor, but result is not written
            0b1010 => panic!("cmp not yet implemented"), // sub, but result is not written
            0b1011 => panic!("cmn not yet implemented"), // add, but result is not written
            0b1100 => self.registers[rd] = op1 | op2, // ORR
            0b1101 => self.registers[rd] = op2, // MOV
            0b1110 => self.registers[rd] = op1 & !op2, // BIC (bit clear)
            _b1111 => self.registers[rd] = !op2, // MVN
        }
    }

    // 4.7 Multiply and Multiply-Accumulate (MUL, MLA)
    // 4.8 Multiply Long and Multiply-Accumulate Long (MULL,MLAL)
    // 4.9 Single Data Transfer (LDR, STR)
    // 4.10 Halfword and Signed Data Transfer
    // 4.11 Block Data Transfer (LDM, STM)
    // 4.12 Single Data Swap (SWP)
    // TODO: implement

    /// 4.13 Software Interrupt (SWI)
    #[inline] fn impl_swi(&mut self, mem: &Memory, op: u32) {
        let _cond       = ((op >> 28) & 0xF);
        let _sel1       = ((op >> 24) & 0xF);
        let _comment    = ((op >>  0) & 0xFFFFFF); // ignored by some/many processors

        debug_assert_ne!(_cond, 0b1111, "invalid cond");
        debug_assert_eq!(_sel1, 0b1111, "swi selector wrong");

        match self.registers[7] {
            1 => { // SC_EXIT
                std::process::exit(self.registers[0] as _);
            },
            4 => { // SC_WRITE
                use std::io::{self, *};

                let fileno      = self.registers[0];
                let mut addr    = self.registers[1];
                let mut size    = self.registers[2] as usize; // not 16-bit safe... but do you think I care?

                let mut stderr : Stderr;
                let mut stdout : Stdout;
                let out : &mut dyn Write = match fileno {
                    1 => { stdout = io::stdout(); &mut stdout },
                    2 => { stderr = io::stderr(); &mut stderr },
                    _ => { self.registers[0] = 9; return }, // r0 = EBADF (Bad file number)
                };

                let mut buffer = [0u8; 512];
                while size > 0 {
                    let read = size.min(buffer.len());
                    mem.read_bytes(addr, MemoryFlags::READ, &mut buffer[..read]);
                    out.write_all(&buffer[..read]).unwrap();
                    addr += read as u32;
                    size -= read;
                }
            },
            _other => {
                panic!("swi #{} - unimplemented SC_??? {}", _comment, self.registers[7]);
            },
        }
    }

    // 4.14 Coprocessor Data Operations (CDP)
    // 4.15 Coprocessor Data Transfers (LDC, STC)
    // 4.16 Coprocessor Register Transfers (MRC, MCR)
    // 4.17 Undefined Instruction
    // TODO: implement
}
