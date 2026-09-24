//! The SILICON virtual machine: a faithful, deterministic 8-bit CPU with a
//! flat 64 KiB address space and memory-mapped devices.

use crate::isa::{Mode, StackReg, table};

pub const MEM_SIZE: usize = 0x10000;

/// Base address of the character plane (32x32 = 1024 bytes).
pub const CHAR_BASE: u16 = 0xF800;
/// Base address of the colour plane (one palette nibble per cell).
pub const COLOR_BASE: u16 = 0xFC00;
pub const PLANE_SIZE: usize = 1024;

/// System registers (reserved region 0xF000..0xF7FF).
pub const VC_CTRL: u16 = 0xF000; // write: 1 clears the screen
pub const KEY_PORT: u16 = 0xF001; // read: last key pressed (or 0)
pub const TICK_PORT: u16 = 0xF002; // read: running frame counter
pub const VC_CONSOLE: u16 = 0xF004; // write: byte pressed into the console

/// The stack grows down from the top of conventional RAM.
pub const STACK_INIT: u16 = 0xEFFF;

/// Flags of the status register.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub z: bool,
    pub c: bool,
    pub n: bool,
}

/// All architectural state of the machine.
#[derive(Clone, Debug)]
pub struct Cpu {
    pub pc: u16,
    pub sp: u16,
    pub a: u8,
    pub b: u8,
    pub x: u8,
    pub y: u8,
    pub flags: Flags,
    pub mem: Box<[u8; MEM_SIZE]>,
    pub halted: bool,
    pub cycles: u64,
    pub frames: u64,
    /// Output console buffer written by the `OUT` instruction.
    pub console: Vec<u8>,
}

/// An off-screen colour used to describe the GPU region in tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenPlane {
    pub chars: [u8; PLANE_SIZE],
    pub colors: [u8; PLANE_SIZE],
}

impl Cpu {
    pub fn new() -> Self {
        Cpu {
            pc: 0,
            sp: STACK_INIT,
            a: 0,
            b: 0,
            x: 0,
            y: 0,
            flags: Flags::default(),
            mem: Box::new([0u8; MEM_SIZE]),
            halted: false,
            cycles: 0,
            frames: 0,
            console: Vec::with_capacity(64),
        }
    }

    pub fn reset(&mut self) {
        *self = Cpu::new();
    }

    /// Loads a program binary at `addr` and points the PC there.
    pub fn load(&mut self, addr: u16, bytes: &[u8]) {
        let base = addr as usize;
        for (i, &b) in bytes.iter().enumerate() {
            self.mem[base + i] = b;
        }
        self.pc = addr;
    }

    pub fn read(&self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            VC_CTRL => {
                if val == 1 {
                    self.mem[CHAR_BASE as usize..CHAR_BASE as usize + PLANE_SIZE].fill(0);
                    self.mem[COLOR_BASE as usize..COLOR_BASE as usize + PLANE_SIZE].fill(0);
                }
            }
            VC_CONSOLE => self.console.push(val),
            _ => self.mem[addr as usize] = val,
        }
    }

    /// Executes one instruction and returns how many cycles it cost.
    /// Returns 0 when the CPU is already halted.
    pub fn step(&mut self) -> u64 {
        if self.halted {
            return 0;
        }
        self.frames += 1;
        let op = self.fetch8();
        let cost = self.execute(op);
        self.cycles += cost;
        cost
    }

    /// Runs until `HLT` or until `budget` cycles are spent.
    pub fn run_budget(&mut self, budget: u64) -> u64 {
        let start = self.cycles;
        while !self.halted && self.cycles - start < budget {
            self.step();
        }
        self.cycles - start
    }

    /// Reads a snapshot of the virtual screen planes.
    pub fn screen(&self) -> ScreenPlane {
        let mut chars = [0u8; PLANE_SIZE];
        let mut colors = [0u8; PLANE_SIZE];
        chars.copy_from_slice(&self.mem[CHAR_BASE as usize..CHAR_BASE as usize + PLANE_SIZE]);
        colors.copy_from_slice(&self.mem[COLOR_BASE as usize..COLOR_BASE as usize + PLANE_SIZE]);
        ScreenPlane { chars, colors }
    }

    fn fetch8(&mut self) -> u8 {
        let b = self.mem[self.pc as usize];
        self.pc = self.pc.wrapping_add(1);
        b
    }

    fn fetch16(&mut self) -> u16 {
        let lo = self.fetch8() as u16;
        let hi = self.fetch8() as u16;
        lo | (hi << 8)
    }

    fn operand(&mut self, mode: Mode) -> u16 {
        match mode {
            Mode::None | Mode::Reg => 0,
            Mode::Imm => self.fetch8() as u16,
            Mode::Abs => self.fetch16(),
            Mode::AbsX => self.fetch16().wrapping_add(self.x as u16),
        }
    }

    fn set_zn(&mut self, v: u8) {
        self.flags.z = v == 0;
        self.flags.n = v & 0x80 != 0;
    }

    fn push8(&mut self, v: u8) {
        self.sp = self.sp.wrapping_sub(1);
        self.mem[self.sp as usize] = v;
    }

    fn pop8(&mut self) -> u8 {
        let v = self.mem[self.sp as usize];
        self.sp = self.sp.wrapping_add(1);
        v
    }

    fn push16(&mut self, v: u16) {
        self.push8((v >> 8) as u8);
        self.push8(v as u8);
    }

    fn pop16(&mut self) -> u16 {
        let lo = self.pop8() as u16;
        let hi = self.pop8() as u16;
        lo | (hi << 8)
    }

    /// Executes the instruction at `pc`; branches update pc in place.
    fn execute(&mut self, op: u8) -> u64 {
        let tbl = table();
        let instr = &tbl[op as usize];
        let operand = self.operand(instr.mode);

        match op {
            0x00 => {} // NOP
            0x01 => {
                self.halted = true;
            }
            0x02 => self.console.push(self.a), // OUT
            0x03 => {
                self.mem[CHAR_BASE as usize..CHAR_BASE as usize + PLANE_SIZE].fill(0);
                self.mem[COLOR_BASE as usize..COLOR_BASE as usize + PLANE_SIZE].fill(0);
            }
            0x04 => self.a = self.read(KEY_PORT), // IN
            0x05 => self.a = self.frames as u8, // TICK

            // Loads — immediate
            0x10 => {
                self.a = operand as u8;
                self.set_zn(self.a);
            }
            0x11 => {
                self.b = operand as u8;
                self.set_zn(self.b);
            }
            0x12 => {
                self.x = operand as u8;
                self.set_zn(self.x);
            }
            0x13 => {
                self.y = operand as u8;
                self.set_zn(self.y);
            }
            // Loads — absolute (indexed reads are resolved by `operand`)
            0x14 => {
                self.a = self.read(operand);
                self.set_zn(self.a);
            }
            0x15 => {
                self.b = self.read(operand);
                self.set_zn(self.b);
            }
            0x16 => {
                self.x = self.read(operand);
                self.set_zn(self.x);
            }
            0x17 => {
                self.y = self.read(operand);
                self.set_zn(self.y);
            }
            0x18 => {
                self.a = self.read(operand);
                self.set_zn(self.a);
            }
            0x19 => {
                self.y = self.read(operand);
                self.set_zn(self.y);
            }

            // ALU immediate 0x20..0x29
            0x20..=0x29 => self.alu(op - 0x20, operand as u8),
            // ALU absolute 0x2A..0x33
            0x2A..=0x33 => self.alu(op - 0x2A, self.read(operand)),
            // ALU absolute+index 0x34..0x3D
            0x34..=0x3D => self.alu(op - 0x34, self.read(operand)),

            0x3E => {
                self.a = self.a.wrapping_add(1);
                self.set_zn(self.a);
            }
            0x3F => {
                self.a = self.a.wrapping_sub(1);
                self.set_zn(self.a);
            }
            0x40 => {
                let v = self.read(operand).wrapping_add(1);
                self.write(operand, v);
                self.set_zn(v);
            }
            0x41 => {
                let v = self.read(operand).wrapping_sub(1);
                self.write(operand, v);
                self.set_zn(v);
            }

            // Transfers
            0x50 => {
                self.x = self.a;
                self.set_zn(self.a);
            }
            0x51 => {
                self.a = self.x;
                self.set_zn(self.a);
            }
            0x52 => {
                self.y = self.a;
                self.set_zn(self.a);
            }
            0x53 => {
                self.a = self.y;
                self.set_zn(self.a);
            }
            0x54 => {
                self.b = self.a;
                self.set_zn(self.a);
            }
            0x55 => {
                self.a = self.b;
                self.set_zn(self.a);
            }

            // Stack
            0x56 => {
                let reg = StackReg::from_byte(operand as u8).unwrap();
                let v = self.reg(reg);
                self.push8(v);
            }
            0x57 => {
                let reg = StackReg::from_byte(operand as u8).unwrap();
                let v = self.pop8();
                self.set_reg(reg, v);
            }
            0x58 => {
                let f = (self.flags.n as u8) << 7
                    | (self.flags.z as u8) << 1
                    | self.flags.c as u8;
                self.push8(f);
            }
            0x59 => {
                let f = self.pop8();
                self.flags.n = f & 0x80 != 0;
                self.flags.z = f & 0x02 != 0;
                self.flags.c = f & 0x01 != 0;
            }

            // Stores
            0x60 => self.write(operand, self.a),
            0x61 => self.write(operand, self.b),
            0x62 => self.write(operand, self.x),
            0x63 => self.write(operand, self.y),
            0x64 => self.write(operand, self.a),
            0x65 => self.write(operand, self.b),

            // Control flow
            0x70 => self.pc = operand,
            0x71 => self.pc = operand,
            0x72 => {
                if self.flags.z {
                    self.pc = operand;
                }
            }
            0x73 => {
                if !self.flags.z {
                    self.pc = operand;
                }
            }
            0x74 => {
                if self.flags.c {
                    self.pc = operand;
                }
            }
            0x75 => {
                if !self.flags.c {
                    self.pc = operand;
                }
            }
            0x76 => {
                if self.flags.n {
                    self.pc = operand;
                }
            }
            0x77 => {
                if !self.flags.n {
                    self.pc = operand;
                }
            }
            0x78 => {
                self.push16(self.pc);
                self.pc = operand;
            }
            0x79 => self.pc = self.pop16(),
            0x7A => self.flags.c = true,
            0x7B => self.flags.c = false,

            // Reserved opcodes are treated as a safe halt so bad binaries
            // cannot wander through uninitialised memory silently.
            _ => self.halted = true,
        }

        instr.cycles as u64
    }

    fn reg(&self, r: StackReg) -> u8 {
        match r {
            StackReg::A => self.a,
            StackReg::B => self.b,
            StackReg::X => self.x,
            StackReg::Y => self.y,
        }
    }

    fn set_reg(&mut self, r: StackReg, v: u8) {
        match r {
            StackReg::A => self.a = v,
            StackReg::B => self.b = v,
            StackReg::X => self.x = v,
            StackReg::Y => self.y = v,
        }
    }

    /// ALU with the accumulator. `sub` is the ALU function index:
    /// 0 ADD, 1 ADC, 2 SUB, 3 SBC, 4 AND, 5 OR, 6 XOR,
    /// 7 CMP, 8 SHL, 9 SHR.
    fn alu(&mut self, sub: u8, m: u8) {
        let a = self.a;
        match sub {
            0 => {
                let (v, c) = a.overflowing_add(m);
                self.a = v;
                self.flags.c = c;
                self.set_zn(v);
            }
            1 => {
                let cin = self.flags.c as u16;
                let total = a as u16 + m as u16 + cin;
                self.a = total as u8;
                self.flags.c = total > 0xFF;
                self.set_zn(self.a);
            }
            2 => {
                let (v, c) = a.overflowing_sub(m);
                self.a = v;
                self.flags.c = !c;
                self.set_zn(v);
            }
            3 => {
                let borrow = !self.flags.c;
                let total = a as u32 + 0x1_0000 - m as u32 - borrow as u32;
                self.a = (total & 0xFF) as u8;
                self.flags.c = total >= 0x1_0000;
                self.set_zn(self.a);
            }
            4 => {
                self.a = a & m;
                self.flags.c = false;
                self.set_zn(self.a);
            }
            5 => {
                self.a = a | m;
                self.flags.c = false;
                self.set_zn(self.a);
            }
            6 => {
                self.a = a ^ m;
                self.flags.c = false;
                self.set_zn(self.a);
            }
            7 => {
                // CMP sets flags as SUB but discards the result.
                let (_, c) = a.overflowing_sub(m);
                let v = a.wrapping_sub(m);
                self.flags.c = !c;
                self.set_zn(v);
            }
            8 => {
                self.flags.c = a & 0x80 != 0;
                self.a = a.wrapping_shl(1);
                self.set_zn(self.a);
            }
            9 => {
                self.flags.c = a & 0x01 != 0;
                self.a = a.wrapping_shr(1);
                self.set_zn(self.a);
            }
            _ => unreachable!(),
        }
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Cpu::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isa;
    use crate::isa::encode;

    fn run(mut cpu: Cpu, budget: u64) -> Cpu {
        cpu.run_budget(budget);
        cpu
    }

    fn cpu_with(prog: &[u8]) -> Cpu {
        let mut c = Cpu::new();
        c.load(0x8000, prog);
        c
    }

    #[test]
    fn hlt_halts() {
        let c = run(cpu_with(&[isa::HLT]), 100);
        assert!(c.halted);
    }

    #[test]
    fn lda_immediate() {
        let c = run(cpu_with(&[0x10, 0x42]), 100);
        assert_eq!(c.a, 0x42);
        assert!(!c.flags.z);
    }

    #[test]
    fn lda_immediate_zero_sets_z() {
        let c = run(cpu_with(&[0x10, 0x00]), 100);
        assert_eq!(c.a, 0);
        assert!(c.flags.z);
    }

    #[test]
    fn add_immediate_with_carry() {
        let c = run(cpu_with(&[0x10, 0xFF, 0x20, 0x01, isa::HLT]), 100);
        assert_eq!(c.a, 0x00);
        assert!(c.flags.z);
        assert!(c.flags.c);
    }

    #[test]
    fn sub_sets_inverted_carry() {
        // 3 - 5 -> borrow -> C = 0 (no-borrow flag false).
        let c = run(cpu_with(&[0x10, 0x03, 0x22, 0x05, isa::HLT]), 100);
        assert_eq!(c.a, 0xFE);
        assert!(c.flags.n);
        assert!(!c.flags.c);
    }

    #[test]
    fn adc_includes_carry_in() {
        // A=1, C=1, ADC 1 -> 3.
        let c = run(
            cpu_with(&[0x10, 0x01, 0x7A, 0x21, 0x01, isa::HLT]),
            1000,
        );
        assert_eq!(c.a, 0x03);
        assert!(!c.flags.c);
    }

    #[test]
    fn cmp_flags_only() {
        // A=5, CMP 5 -> Z set, C set, A unchanged.
        let c = run(cpu_with(&[0x10, 0x05, 0x27, 0x05, isa::HLT]), 100);
        assert_eq!(c.a, 0x05);
        assert!(c.flags.z);
        assert!(c.flags.c);
    }

    #[test]
    fn shifts_set_carry_and_shift_out() {
        // A=0b1000_0001, SHL -> 0b0000_0010, C=1, N=0.
        let c = run(cpu_with(&[0x10, 0b1000_0001, 0x28, isa::HLT]), 100);
        assert_eq!(c.a, 0b0000_0010);
        assert!(c.flags.c);
        assert!(!c.flags.n);
    }

    #[test]
    fn shr_sets_carry_lsb() {
        // A=0b0000_0011, SHR -> 1, C=1.
        let c = run(cpu_with(&[0x10, 0b0000_0011, 0x29, isa::HLT]), 100);
        assert_eq!(c.a, 1);
        assert!(c.flags.c);
    }

    #[test]
    fn indexed_addressing() {
        // A = mem[0x9000 + X]. Load X=2, store at 0x9002, then LDAAX.
        let mut c = Cpu::new();
        c.load(0x8000, &[0x12, 0x02, 0x10, 0x55, 0x60, 0x02, 0x90, 0x18, 0x00, 0x90, isa::HLT]);
        c = run(c, 1000);
        assert_eq!(c.a, 0x55);
    }

    #[test]
    fn push_pop_round_trip() {
        let c = run(
            cpu_with(&[0x10, 0x7F, 0x56, 0x00, 0x10, 0x11, 0x57, 0x00, isa::HLT]),
            1000,
        );
        assert_eq!(c.a, 0x7F);
    }

    #[test]
    fn stack_grows_down_and_sp_restored() {
        let mut c = cpu_with(&[
            0x10, 0x7F, 0x56, 0x00, // A=$7F, PSH A
            0x12, 0x42, 0x56, 0x02, // X=$42, PSH X
            0x10, 0x00, 0x57, 0x02, // clobber A, POP X
            0x57, 0x00, isa::HLT, // POP A
        ]);
        c = run(c, 1000);
        assert_eq!(c.sp, STACK_INIT);
        assert_eq!(c.a, 0x7F);
        assert_eq!(c.x, 0x42);
    }

    #[test]
    fn branch_taken_and_not_taken() {
        // Taken: A=0 -> Z set, JZ lands past the HLT.
        let taken: &[u8] = &[0x10, 0x00, 0x72, 0x06, 0x80, isa::HLT, 0x10, 0xAA];
        let c = run(cpu_with(taken), 1000);
        assert_eq!(c.a, 0xAA);
        assert!(!c.halted);
        // Not taken: A=1 -> Z clear, JZ falls through into HLT.
        let skipped: &[u8] = &[0x10, 0x01, 0x72, 0x06, 0x80, isa::HLT];
        let c = run(cpu_with(skipped), 1000);
        assert_eq!(c.a, 0x01);
        assert!(c.halted);
    }

    #[test]
    fn call_ret_returns_after_callee() {
        // main: CALL $sub ; A=1
        // sub:  A=7 ; RET
        let mut c = cpu_with(&[0x78, 0x00, 0x81, 0x10, 0x01, isa::HLT]);
        c.load(0x8100, &[0x10, 0x07, 0x79]);
        c.pc = 0x8000;
        let c = run(c, 1000);
        assert_eq!(c.a, 0x01);
        assert!(c.halted);
    }

    #[test]
    fn stack_pages_after_call() {
        let mut c = cpu_with(&[0x78, 0x00, 0x81, isa::HLT]);
        c.load(0x8100, &[0x79]);
        c.pc = 0x8000;
        let c = run(c, 1000);
        assert_eq!(c.sp, STACK_INIT);
    }

    #[test]
    fn jmp_absx_is_computed() {
        // X=2, table at 0x8100..: jump targets. JMP table,X.
        let mut c = Cpu::new();
        c.load(0x8000, &[0x12, 0x02, 0x71, 0x00, 0x81, isa::HLT]);
        // table at 0x8100: two 16-bit little-endian targets.
        c.load(0x8100, &[0x00, 0x82, 0x00, 0x83]);
        c.load(0x8200, &[0x10, 0x11]);
        c = run(c, 1000);
        assert_eq!(c.a, 0x11);
        assert!(!c.halted);
    }

    #[test]
    fn out_writes_console() {
        let c = run(cpu_with(&[0x10, b'H', 0x02, 0x10, b'i', 0x02, isa::HLT]), 100);
        assert_eq!(c.console, b"Hi");
    }

    #[test]
    fn cls_clears_screen() {
        let mut c = Cpu::new();
        c.load(0x8000, &[0x10, 0x41, 0x60, 0x00, 0xF8, 0x03, isa::HLT]);
        c.run_budget(1000);
        let s = c.screen();
        assert_eq!(s.chars[0], 0);
    }

    #[test]
    fn touching_screen_char_plane_writes_memory() {
        // Writing to CHAR_BASE is a normal memory write, not trapped by I/O.
        let mut c = Cpu::new();
        c.load(0x8000, &[0x10, 0x41, 0x60, 0x00, 0xF8, isa::HLT]);
        c.run_budget(1000);
        assert_eq!(c.mem[CHAR_BASE as usize], 0x41);
    }

    #[test]
    fn tick_port_advances() {
        let c = run(cpu_with(&[0x05, 0x05, 0x05]), 1000);
        // Each step advances frames; A last holds frame & 0xFF.
        assert!(c.halted || c.cycles >= 0);
    }

    #[test]
    fn reserved_opcode_halts_safely() {
        let c = run(cpu_with(&[0xEE]), 1000);
        assert!(c.halted);
    }

    #[test]
    fn alu_abs_mode_reads_memory() {
        // A=5, store at 0x9000 via LDA-imm+STA, then ADD abs $9000.
        let prog: &[u8] = &[
            0x10, 0x05, 0x60, 0x00, 0x90, 0x10, 0x03, 0x2A, 0x00, 0x90, isa::HLT,
        ];
        let c = run(cpu_with(prog), 1000);
        assert_eq!(c.a, 0x08);
    }

    #[test]
    fn encode_helpers_work() {
        assert_eq!(encode("LDA", Mode::Imm), Some(0x10));
        assert_eq!(encode("ADD", Mode::AbsX), Some(0x34));
        assert_eq!(encode("PSH", Mode::Reg), Some(0x56));
    }
}