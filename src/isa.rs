//! Instruction set of the SILICON machine.
//!
//! A single flat table of 256 opcodes. Each opcode carries its mnemonic,
//! its addressing mode (which implies the encoded length) and a base cost in
//! cycles. This table is the single source of truth used by the emulator, the
//! assembler and the disassembler, so the three can never drift apart.

/// Addressing mode. The mode decides how many bytes an instruction occupies.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Mode {
    /// No operand. 1 byte.
    None,
    /// `#imm` — 8-bit operand packed in the following byte. 2 bytes.
    Imm,
    /// `addr` — 16-bit little-endian absolute address. 3 bytes.
    Abs,
    /// `addr,X` — 16-bit absolute address plus the 8-bit X index. 3 bytes.
    AbsX,
    /// `<reg>` — register selector byte (0=A, 1=B, 2=X, 3=Y). 2 bytes.
    Reg,
}

impl Mode {
    /// Encoded length in bytes (opcode included).
    pub fn len(self) -> usize {
        match self {
            Mode::None => 1,
            Mode::Imm | Mode::Reg => 2,
            Mode::Abs | Mode::AbsX => 3,
        }
    }
}

/// One entry of the opcode table.
#[derive(Clone, Copy, Debug)]
pub struct Instr {
    pub name: &'static str,
    pub mode: Mode,
    pub cycles: u8,
}

pub const NOP: u8 = 0x00;
pub const HLT: u8 = 0x01;

/// Builds the 256-entry opcode table.
pub fn table() -> [Instr; 256] {
    let mut t: [Instr; 256] = [Instr { name: "RES", mode: Mode::None, cycles: 1 }; 256];

    let mut set = |op: u8, name: &'static str, mode: Mode, cycles: u8| t[op as usize] = Instr { name, mode, cycles };

    // --- System -------------------------------------------------------------
    set(NOP, "NOP", Mode::None, 2);
    set(HLT, "HLT", Mode::None, 1);
    set(0x02, "OUT", Mode::None, 2); // write A to the console
    set(0x03, "CLS", Mode::None, 2); // clear both screen planes
    set(0x04, "IN", Mode::None, 2); // A = key port
    set(0x05, "TICK", Mode::None, 2); // A = frame counter (low byte)

    // --- Loads --------------------------------------------------------------
    // LDA / LDB / LDX / LDY — immediate, absolute, absolute+index.
    set(0x10, "LDA", Mode::Imm, 3);
    set(0x11, "LDB", Mode::Imm, 3);
    set(0x12, "LDX", Mode::Imm, 3);
    set(0x13, "LDY", Mode::Imm, 3);
    set(0x14, "LDA", Mode::Abs, 4);
    set(0x15, "LDB", Mode::Abs, 4);
    set(0x16, "LDX", Mode::Abs, 4);
    set(0x17, "LDY", Mode::Abs, 4);
    set(0x18, "LDA", Mode::AbsX, 5);
    set(0x19, "LDY", Mode::AbsX, 5);

    // --- ALU with A ---------------------------------------------------------
    // Immediate: 0x20..0x29
    for (i, name) in ["ADD", "ADC", "SUB", "SBC", "AND", "OR", "XOR", "CMP", "SHL", "SHR"]
        .iter()
        .enumerate()
    {
        set(0x20 + i as u8, name, Mode::Imm, 3);
    }
    // Absolute: 0x2A..0x33
    for (i, name) in ["ADD", "ADC", "SUB", "SBC", "AND", "OR", "XOR", "CMP", "SHL", "SHR"]
        .iter()
        .enumerate()
    {
        set(0x2A + i as u8, name, Mode::Abs, 4);
    }
    // Absolute+index: 0x34..0x3D
    for (i, name) in ["ADD", "ADC", "SUB", "SBC", "AND", "OR", "XOR", "CMP", "SHL", "SHR"]
        .iter()
        .enumerate()
    {
        set(0x34 + i as u8, name, Mode::AbsX, 5);
    }

    // --- Increment / decrement ----------------------------------------------
    set(0x3E, "INC", Mode::None, 2); // INC A
    set(0x3F, "DEC", Mode::None, 2); // DEC A
    set(0x40, "INC", Mode::Abs, 5);
    set(0x41, "DEC", Mode::Abs, 5);

    // --- Transfers ----------------------------------------------------------
    set(0x50, "TAX", Mode::None, 2);
    set(0x51, "TXA", Mode::None, 2);
    set(0x52, "TAY", Mode::None, 2);
    set(0x53, "TYA", Mode::None, 2);
    set(0x54, "TAB", Mode::None, 2);
    set(0x55, "TBA", Mode::None, 2);

    // --- Stack --------------------------------------------------------------
    // PSH/POP take a register selector byte: 0=A, 1=B, 2=X, 3=Y.
    set(0x56, "PSH", Mode::Reg, 3);
    set(0x57, "POP", Mode::Reg, 3);
    set(0x58, "PSHF", Mode::None, 2); // push flags
    set(0x59, "POPF", Mode::None, 2); // pop flags

    // --- Stores -------------------------------------------------------------
    set(0x60, "STA", Mode::Abs, 4);
    set(0x61, "STB", Mode::Abs, 4);
    set(0x62, "STX", Mode::Abs, 4);
    set(0x63, "STY", Mode::Abs, 4);
    set(0x64, "STA", Mode::AbsX, 5);
    set(0x65, "STB", Mode::AbsX, 5);

    // --- Control flow -------------------------------------------------------
    set(0x70, "JMP", Mode::Abs, 3);
    set(0x71, "JMP", Mode::AbsX, 5);
    set(0x72, "JZ", Mode::Abs, 3);
    set(0x73, "JNZ", Mode::Abs, 3);
    set(0x74, "JC", Mode::Abs, 3);
    set(0x75, "JNC", Mode::Abs, 3);
    set(0x76, "JN", Mode::Abs, 3);
    set(0x77, "JNN", Mode::Abs, 3);
    set(0x78, "CALL", Mode::Abs, 4);
    set(0x79, "RET", Mode::None, 4);
    set(0x7A, "SEC", Mode::None, 2);
    set(0x7B, "CLC", Mode::None, 2);

    t
}

/// Looks up the opcode for a mnemonic + addressing mode. Used by the
/// assembler. Returns `None` when the combination does not exist.
pub fn encode(name: &str, mode: Mode) -> Option<u8> {
    let t = table();
    for (op, instr) in t.iter().enumerate() {
        if instr.name == name && instr.mode == mode {
            return Some(op as u8);
        }
    }
    None
}

/// Register selector byte used by `PSH`/`POP`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackReg {
    A,
    B,
    X,
    Y,
}

impl StackReg {
    pub fn selector(self) -> u8 {
        match self {
            StackReg::A => 0,
            StackReg::B => 1,
            StackReg::X => 2,
            StackReg::Y => 3,
        }
    }

    pub fn from_byte(b: u8) -> Option<StackReg> {
        match b {
            0 => Some(StackReg::A),
            1 => Some(StackReg::B),
            2 => Some(StackReg::X),
            3 => Some(StackReg::Y),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            StackReg::A => "A",
            StackReg::B => "B",
            StackReg::X => "X",
            StackReg::Y => "Y",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_consistent() {
        let t = table();
        // Every entry has a positive cost and a name.
        for op in 0..=255u8 {
            let instr = &t[op as usize];
            assert!(instr.cycles > 0, "opcode ${op:02X} has zero cycles");
            assert!(!instr.name.is_empty());
        }
    }

    #[test]
    fn no_duplicate_name_mode_pairs() {
        // If two opcodes shared a name+mode the assembler could not pick one.
        let mut seen = std::collections::HashSet::new();
        for instr in table().iter() {
            let key = (instr.name, instr.mode);
            if instr.name == "RES" {
                continue;
            }
            assert!(
                seen.insert(key),
                "duplicate name+mode: {} {:?}",
                instr.name,
                instr.mode
            );
        }
    }

    #[test]
    fn encoding_round_trip() {
        let t = table();
        for (op, instr) in t.iter().enumerate() {
            if instr.name == "RES" {
                continue;
            }
            assert_eq!(encode(instr.name, instr.mode), Some(op as u8));
        }
    }

    #[test]
    fn unknown_instruction_is_none() {
        assert_eq!(encode("FLY", Mode::None), None);
        assert_eq!(encode("LDA", Mode::None), None);
    }

    #[test]
    fn lengths_match_binary_layout() {
        assert_eq!(Mode::None.len(), 1);
        assert_eq!(Mode::Imm.len(), 2);
        assert_eq!(Mode::Abs.len(), 3);
        assert_eq!(Mode::AbsX.len(), 3);
    }
}