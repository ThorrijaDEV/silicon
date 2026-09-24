//! Disassembler: turns a binary back into readable SILICON assembly.
//!
//! Two output flavours:
//!  - a listing with address and raw bytes (for humans, `disasm`),
//!  - a re-assemblable source (`-o out.asm`), which round-trips byte-for-byte
//!    through the assembler for pure-code images.

use crate::isa::{Mode, StackReg, table};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub addr: u16,
    pub bytes: Vec<u8>,
    pub text: String,
}

/// Decodes a binary image starting at `base`.
pub fn decode(bytes: &[u8], base: u16) -> Vec<Decoded> {
    let mut out = vec![];
    let tbl = table();
    let mut addr = base;
    let mut i = 0;
    while i < bytes.len() {
        let op = bytes[i];
        let instr = &tbl[op as usize];
        let len = instr.mode.len();
        let end = (i + len).min(bytes.len());
        let raw = &bytes[i..end];
        let text = fmt(op, raw);
        out.push(Decoded { addr, bytes: raw.to_vec(), text });
        addr = addr.wrapping_add(len as u16);
        i = end;
    }
    out
}

/// Formats one instruction in canonical assembly (`LDA #$41`, `JMP $8107`).
pub fn fmt(op: u8, raw: &[u8]) -> String {
    let tbl = table();
    let instr = &tbl[op as usize];
    match instr.mode {
        Mode::None => match op {
            0x3E | 0x3F => format!("{} A", instr.name),
            _ => instr.name.to_string(),
        },
        Mode::Imm => {
            let v = raw.get(1).copied().unwrap_or(0);
            format!("{} #${v:02X}", instr.name)
        }
        Mode::Abs | Mode::AbsX => {
            let lo = raw.get(1).copied().unwrap_or(0);
            let hi = raw.get(2).copied().unwrap_or(0);
            let v = lo as u16 | ((hi as u16) << 8);
            if instr.mode == Mode::AbsX {
                format!("{} ${v:04X},X", instr.name)
            } else {
                format!("{} ${v:04X}", instr.name)
            }
        }
        Mode::Reg => {
            let sel = raw.get(1).copied().unwrap_or(0);
            let reg = StackReg::from_byte(sel).unwrap_or(StackReg::A).name();
            format!("{} {}", instr.name, reg)
        }
    }
}

/// A human-friendly listing with an address column and raw bytes.
pub fn listing(bytes: &[u8], base: u16) -> String {
    let mut sb = String::with_capacity(bytes.len() * 24);
    for d in decode(bytes, base) {
        let hexbytes = d
            .bytes
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" ");
        sb.push_str(&format!("${:04X}:  {:<11} {}\n", d.addr, hexbytes, d.text));
    }
    sb
}

/// A re-assemblable source: `.origin` header followed by one mnemonic per
/// line. Re-assembles to the identical image when the region contains no raw
/// data.
pub fn source(bytes: &[u8], base: u16) -> String {
    let mut sb = String::with_capacity(bytes.len() * 16);
    sb.push_str(&format!(".origin ${base:04X}\n"));
    for d in decode(bytes, base) {
        sb.push_str(&d.text);
        sb.push('\n');
    }
    sb
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembler::assemble_str;

    #[test]
    fn decode_simple() {
        let bytes = [0x10, 0x41, 0x02, 0x01];
        let de = decode(&bytes, 0x8000);
        assert_eq!(de.len(), 3);
        assert_eq!(de[0].text, "LDA #$41");
        assert_eq!(de[1].text, "OUT");
        assert_eq!(de[2].text, "HLT");
        assert_eq!(de[0].addr, 0x8000);
        assert_eq!(de[1].addr, 0x8002);
    }

    #[test]
    fn decode_abs_and_reg() {
        let bytes = [0x70, 0x34, 0x12, 0x56, 0x02, 0x57, 0x03];
        let de = decode(&bytes, 0);
        assert_eq!(de[0].text, "JMP $1234");
        assert_eq!(de[1].text, "PSH X");
        assert_eq!(de[2].text, "POP Y");
    }

    #[test]
    fn decode_absx() {
        let bytes = [0x64, 0x00, 0xF0];
        let de = decode(&bytes, 0);
        assert_eq!(de[0].text, "STA $F000,X");
    }

    #[test]
    fn round_trip_pure_code() {
        let src = r#"
            .origin $8000
            LDA #$00
            loop:
              CMP #$0A
              JZ done
              OUT
              INC A
              JMP loop
            done:
              HLT
        "#;
        let p = assemble_str(src).unwrap();
        let dis = source(&p.bytes, p.entry);
        let again = assemble_str(&dis).unwrap();
        assert_eq!(p.bytes, again.bytes);
    }

    #[test]
    fn listing_contains_addresses() {
        let l = listing(&[0x10, 0x00], 0x8000);
        assert!(l.contains("$8000"));
        assert!(l.contains("10 00"));
        assert!(l.contains("LDA #$00"));
    }

#[test]
    fn partial_last_instruction_does_not_panic() {
        // Truncated operand bytes expand to zero without panicking.
        let de = decode(&[0x70, 0x34], 0);
        assert_eq!(de.len(), 1);
        assert_eq!(de[0].text, "JMP $0034");
    }
}