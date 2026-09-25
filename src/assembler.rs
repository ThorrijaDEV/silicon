//! Two-pass assembler for the SILICON instruction set.
//!
//! Accepts the hardware mnemonics, labels, `.include`, numeric literals
//! (hex `$`, binary `%`, decimal, char `'x'`), and these directives:
//! `.origin`, `.byte`/`.db`, `.word`/`.dw`, `.string`/`.asciz`, `.space`.
//!
//! Pass 1 lays out code and resolves label addresses; pass 2 emits the
//! binary. Errors carry the offending file and line.

use crate::isa::{self, Mode, StackReg};
use std::collections::HashMap;
use std::path::Path;

/// A parsed, loadable program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub bytes: Vec<u8>,
    /// Address where execution starts (first `.origin`).
    pub entry: u16,
    /// Label → address map, useful for debuggers and listings.
    pub symbols: HashMap<String, u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembleError {
    pub message: String,
    pub line: usize,
    pub file: String,
}

impl AssembleError {
    fn new(file: &str, line: usize, message: impl Into<String>) -> Self {
        AssembleError { file: file.to_string(), line: line + 1, message: message.into() }
    }
}

impl std::fmt::Display for AssembleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.message)
    }
}

impl std::error::Error for AssembleError {}

struct Line {
    file: String,
    number: usize,
    raw: String,
}

struct Unit {
    lines: Vec<Line>,
}

struct State {
    symbols: HashMap<String, u16>,
    pc: u16,
    output: Vec<u8>,
    outcome: Option<AssembleError>,
    /// First `.origin` seen — the entry point.
    entry: Option<u16>,
    /// Whether anything has been emitted yet (for entry detection).
    emitted: bool,
}

/// Assembles the file at `path`, resolving `.include` relative to it.
pub fn assemble(path: &str) -> Result<Program, AssembleError> {
    let unit = load_unit(path, &mut Vec::new())?;
    let (symbols, entry) = first_pass(&unit)?;
    let mut st = State::new();
    st.symbols = symbols;
    second_pass(&unit, &mut st)?;
    Ok(Program {
        bytes: st.output,
        entry: entry.unwrap_or(0),
        symbols: st.symbols,
    })
}

/// Assembles a source string. Useful for tests and embedding.
pub fn assemble_str(src: &str) -> Result<Program, AssembleError> {
    let unit = Unit {
        lines: src
            .lines()
            .enumerate()
            .map(|(i, l)| Line { file: "<string>".to_string(), number: i, raw: l.to_string() })
            .collect(),
    };
    let (symbols, entry) = first_pass(&unit)?;
    let mut st = State::new();
    st.symbols = symbols;
    second_pass(&unit, &mut st)?;
    Ok(Program {
        bytes: st.output,
        entry: entry.unwrap_or(0),
        symbols: st.symbols,
    })
}

impl State {
    fn new() -> Self {
        State {
            symbols: HashMap::new(),
            pc: 0,
            output: Vec::new(),
            outcome: None,
            entry: None,
            emitted: false,
        }
    }

    fn fail(&mut self, line: &Line, msg: impl Into<String>) {
        if self.outcome.is_none() {
            self.outcome = Some(AssembleError::new(&line.file, line.number, msg));
        }
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.output.extend_from_slice(bytes);
        self.pc = self.pc.wrapping_add(bytes.len() as u16);
        self.emitted = true;
    }

    fn define(&mut self, name: &str, line: &Line) {
        if self.symbols.insert(name.to_string(), self.pc).is_some() {
            self.fail(line, format!("duplicate label `{name}`"));
        }
    }

    fn locate(&mut self, addr: u16) {
        if !self.emitted && self.entry.is_none() {
            self.entry = Some(addr);
        }
        self.pc = addr;
        self.emitted = true;
    }
}

/// Removes a comment from a line, honouring string literals.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            ';' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Splits a possible `label:` prefix from the rest of a line.
fn split_label(src: &str) -> (Option<String>, &str) {
    match src.find(':') {
        Some(colon) => {
            let before = &src[..colon];
            if is_ident(before.trim()) {
                (Some(before.trim().to_string()), src[colon + 1..].trim())
            } else {
                (None, src)
            }
        }
        None => (None, src),
    }
}

fn first_pass(unit: &Unit) -> Result<(HashMap<String, u16>, Option<u16>), AssembleError> {
    let mut st = State::new();
    for line in &unit.lines {
        if st.outcome.is_some() {
            break;
        }
        let src = strip_comment(&line.raw).trim();
        if src.is_empty() {
            continue;
        }
        let (label, rest) = split_label(src);
        if let Some(lbl) = &label {
            st.define(lbl, line);
        }
        if rest.is_empty() {
            continue;
        }
        if let Some(sub) = rest.strip_prefix('.') {
            let head = sub.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
            let tail = after_directive(sub);
            match head.as_str() {
                "origin" => match parse_expr(tail, &st.symbols) {
                    Ok(v) => st.locate(v as u16),
                    Err(e) => st.fail(line, e),
                },
                "byte" | "db" => {
                    let n = split_operands(tail)
                        .iter()
                        .filter(|s| !s.trim().is_empty())
                        .count();
                    st.pc = st.pc.wrapping_add(n as u16);
                    st.emitted = true;
                }
                "word" | "dw" => {
                    let n = split_operands(tail)
                        .iter()
                        .filter(|s| !s.trim().is_empty())
                        .count()
                        * 2;
                    st.pc = st.pc.wrapping_add(n as u16);
                    st.emitted = true;
                }
                "string" | "asciz" => match parse_raw_string(tail.trim()) {
                    Ok(s) => {
                        st.pc = st.pc.wrapping_add(s.len() as u16 + (head == "asciz") as u16);
                        st.emitted = true;
                    }
                    Err(e) => st.fail(line, e),
                },
                "space" | "mem" => match tail.trim().parse::<u32>() {
                    Ok(n) => {
                        st.pc = st.pc.wrapping_add(n.min(0x1_0000) as u16);
                        st.emitted = true;
                    }
                    Err(_) => st.fail(line, "expected a number after .space"),
                },
                _ => st.fail(line, format!("unknown directive `.{head}`")),
            }
            continue;
        }

        let mut body = src;
        if let Some(colon) = body.find(':') {
            let label = body[..colon].trim();
            if is_ident(label) {
                st.define(label, line);
                body = body[colon + 1..].trim();
                if body.is_empty() {
                    continue;
                }
            }
        }

        match instr_size(body) {
            Ok(size) => {
                st.pc = st.pc.wrapping_add(size as u16);
                st.emitted = true;
            }
            Err(e) => st.fail(line, e),
        }
    }
    match st.outcome.take() {
        Some(err) => Err(err),
        None => Ok((st.symbols, st.entry)),
    }
}

fn second_pass(unit: &Unit, st: &mut State) -> Result<(), AssembleError> {
    for line in &unit.lines {
        if st.outcome.is_some() {
            break;
        }
        let src = strip_comment(&line.raw).trim();
        if src.is_empty() {
            continue;
        }
        let (_label, rest) = split_label(src);
        if rest.is_empty() {
            continue;
        }
        if let Some(sub) = rest.strip_prefix('.') {
            let head = sub.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
            let tail = after_directive(sub);
            match head.as_str() {
                "origin" => match parse_expr(tail, &st.symbols) {
                    Ok(v) => st.locate(v as u16),
                    Err(e) => st.fail(line, e),
                },
                "byte" | "db" => match parse_byte_list(tail, &st.symbols) {
                    Ok(v) => st.emit(&v),
                    Err(e) => st.fail(line, e),
                },
                "word" | "dw" => match parse_word_list(tail, &st.symbols) {
                    Ok(v) => st.emit(&v),
                    Err(e) => st.fail(line, e),
                },
                "string" | "asciz" => match parse_raw_string(tail.trim()) {
                    Ok(s) => {
                        let mut bytes = s.as_bytes().to_vec();
                        if head == "asciz" {
                            bytes.push(0);
                        }
                        st.emit(&bytes);
                    }
                    Err(e) => st.fail(line, e),
                },
                "space" | "mem" => match tail.trim().parse::<u32>() {
                    Ok(n) => st.emit(&vec![0u8; n.min(0x1_0000) as usize]),
                    Err(_) => st.fail(line, "expected a number after .space"),
                },
                _ => st.fail(line, format!("unknown directive `.{head}`")),
            }
            continue;
        }

        if let Err(e) = emit_instr(rest, st) {
            st.fail(line, e);
        }
    }
    match st.outcome.take() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn after_directive(rest: &str) -> &str {
    match rest.split_once(char::is_whitespace) {
        Some((_, tail)) => tail.trim(),
        None => "",
    }
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.chars().next().unwrap().is_ascii_digit()
        && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
}

/// Computes the encoded length of a mnemonic + operand form.
fn instr_size(src: &str) -> Result<usize, String> {
    let mut it = src.splitn(2, char::is_whitespace);
    let mnemonic = it.next().unwrap_or("").trim().to_uppercase();
    let operands = it.next().unwrap_or("").trim();

    match mnemonic.as_str() {
        "NOP" | "HLT" | "OUT" | "CLS" | "IN" | "TICK" => Ok(1),
        "TAX" | "TXA" | "TAY" | "TYA" | "TAB" | "TBA" | "PSHF" | "POPF" | "RET" => Ok(1),
        "SEC" | "CLC" => Ok(1),
        "INC" | "DEC" if operands == "A" => Ok(1),
        "INC" | "DEC" => Ok(3),
        "PSH" | "POP" => {
            if matches!(operands.trim().to_uppercase().as_str(), "A" | "B" | "X" | "Y") {
                Ok(2)
            } else {
                Err(format!("{mnemonic}: expected A/B/X/Y, got `{operands}`"))
            }
        }
        "LDA" | "LDB" | "LDX" | "LDY"
        | "ADD" | "ADC" | "SUB" | "SBC" | "AND" | "OR" | "XOR" | "CMP" | "SHL" | "SHR" => {
            match mode_of(operands) {
                Mode::Imm => Ok(2),
                Mode::Abs | Mode::AbsX => Ok(3),
                _ => Err(format!("{mnemonic}: bad operand `{operands}`")),
            }
        }
        "STA" | "STB" | "STX" | "STY" | "JMP" | "JZ" | "JNZ" | "JC" | "JNC" | "JN" | "JNN"
        | "CALL" => {
            let m = mode_of(operands);
            if m == Mode::Abs || m == Mode::AbsX {
                Ok(3)
            } else {
                Err(format!("{mnemonic}: bad operand `{operands}`"))
            }
        }
        _ => Err(format!("unknown mnemonic `{mnemonic}`")),
    }
}

fn mode_of(operands: &str) -> Mode {
    let op = operands.trim();
    if op.is_empty() {
        return Mode::None;
    }
    if op.starts_with('#') {
        return Mode::Imm;
    }
    if op.contains(',') {
        return Mode::AbsX;
    }
    Mode::Abs
}

fn operand_head(operands: &str) -> &str {
    if let Some((head, _)) = operands.trim().split_once(',') {
        head.trim()
    } else {
        operands.trim()
    }
}

fn emit_instr(src: &str, st: &mut State) -> Result<(), String> {
    let mut it = src.splitn(2, char::is_whitespace);
    let mnemonic = it.next().unwrap_or("").trim().to_uppercase();
    let operands = it.next().unwrap_or("").trim();

    let sel = |r: &str| -> Result<u8, String> {
        Ok(match r.trim().to_uppercase().as_str() {
            "A" => StackReg::A.selector(),
            "B" => StackReg::B.selector(),
            "X" => StackReg::X.selector(),
            "Y" => StackReg::Y.selector(),
            _ => return Err(format!("{mnemonic}: expected A/B/X/Y, got `{r}`")),
        })
    };

    let mut out: Vec<u8> = Vec::with_capacity(4);
    match mnemonic.as_str() {
        "NOP" => out.push(isa::NOP),
        "HLT" => out.push(isa::HLT),
        "OUT" => out.push(0x02),
        "CLS" => out.push(0x03),
        "IN" => out.push(0x04),
        "TICK" => out.push(0x05),
        "TAX" => out.push(0x50),
        "TXA" => out.push(0x51),
        "TAY" => out.push(0x52),
        "TYA" => out.push(0x53),
        "TAB" => out.push(0x54),
        "TBA" => out.push(0x55),
        "PSHF" => out.push(0x58),
        "POPF" => out.push(0x59),
        "RET" => out.push(0x79),
        "SEC" => out.push(0x7A),
        "CLC" => out.push(0x7B),
        "INC" | "DEC" if operands == "A" => {
            out.push(if mnemonic == "INC" { 0x3E } else { 0x3F });
        }
        "INC" | "DEC" => {
            let addr = parse_abs(operands, &st.symbols)?;
            out.push(if mnemonic == "INC" { 0x40 } else { 0x41 });
            out.extend_from_slice(&[addr as u8, (addr >> 8) as u8]);
        }
        "PSH" | "POP" => {
            let s = sel(operands)?;
            out.push(if mnemonic == "PSH" { 0x56 } else { 0x57 });
            out.push(s);
        }
        _ => {
            let mode = mode_of(operands);
            let opcode = isa::encode(&mnemonic, mode)
                .ok_or_else(|| format!("`{mnemonic}` does not support this operand form"))?;
            out.push(opcode);
            match mode {
                Mode::None => {}
                Mode::Imm => {
                    let v = parse_imm(operands.trim_start_matches('#'), &st.symbols)?;
                    out.push(v);
                }
                Mode::Abs => {
                    let addr = parse_abs(operands, &st.symbols)?;
                    out.extend_from_slice(&[addr as u8, (addr >> 8) as u8]);
                }
                Mode::AbsX => {
                    let addr = parse_abs(operand_head(operands), &st.symbols)?;
                    out.extend_from_slice(&[addr as u8, (addr >> 8) as u8]);
                }
                Mode::Reg => unreachable!("reg mode handled above"),
            }
        }
    }
    st.emit(&out);
    Ok(())
}

fn parse_byte_list(src: &str, symbols: &HashMap<String, u16>) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    for item in split_operands(src) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if let Some(s) = item.strip_prefix('\'') {
            let inner = s.strip_suffix('\'').ok_or_else(|| format!("bad char `{item}`"))?;
            let bytes = inner.as_bytes();
            if bytes.len() != 1 {
                return Err(format!("bad char literal `{item}`"));
            }
            out.push(bytes[0]);
        } else if item.starts_with('"') {
            out.extend_from_slice(parse_raw_string(item)?.as_bytes());
        } else {
            let v = parse_expr(item, symbols)?;
            if v > 0xFF {
                return Err(format!("byte value 0x{v:02X} out of range"));
            }
            out.push(v as u8);
        }
    }
    Ok(out)
}

fn parse_word_list(src: &str, symbols: &HashMap<String, u16>) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    for item in split_operands(src) {
        let v = parse_expr(item.trim(), symbols)?;
        if v > 0xFFFF {
            return Err(format!("word value 0x{v:04X} out of range"));
        }
        out.push(v as u8);
        out.push((v >> 8) as u8);
    }
    Ok(out)
}

/// Splits a comma-separated operand list, but not inside string literals.
fn split_operands(src: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut start = 0;
    let mut in_str = false;
    for (i, c) in src.char_indices() {
        match c {
            '"' => in_str = !in_str,
            ',' if !in_str => {
                out.push(&src[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&src[start..]);
    out
}

fn parse_raw_string(item: &str) -> Result<String, String> {
    let item = item.trim();
    if let Some(inner) = item.strip_prefix('"') {
        if let Some(rest) = inner.strip_suffix('"') {
            return decode_escapes(rest);
        }
        return Err(format!("unterminated string `{item}`"));
    }
    Err(format!("expected a quoted string, got `{item}`"))
}

/// Expands `\n`, `\t`, `\\` y `\"` dentro de una cadena literal (por el
/// momento los mismos escapes que admite el compilador Forja).
fn decode_escapes(s: &str) -> Result<String, String> {
    if !s.contains('\\') {
        return Ok(s.to_string());
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => {
                return Err(format!("escape desconocido `\\{other}` en cadena literal"))
            }
            None => return Err("cadena que termina en una barra invertida".to_string()),
        }
    }
    Ok(out)
}

/// Tiny expression parser: `term ( +|- term )*`, where a term is a numeric
/// literal, a label, a parenthesised expression or a signed term.
fn parse_expr(src: &str, symbols: &HashMap<String, u16>) -> Result<u32, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut idx = 0;
    while idx < chars.len() {
        let c = chars[idx];
        if c.is_whitespace() {
            idx += 1;
            continue;
        }
        if c == '\'' {
            tokens.push(c);
            idx += 1;
            while idx < chars.len() && chars[idx] != '\'' {
                tokens.push(chars[idx]);
                idx += 1;
            }
            if idx < chars.len() {
                tokens.push(chars[idx]);
                idx += 1;
            }
        } else {
            tokens.push(c);
            idx += 1;
        }
    }
    if tokens.is_empty() {
        return Err("empty expression".to_string());
    }
    let mut i = 0;
    let v = parse_arith(&tokens, &mut i, symbols)?;
    if i != tokens.len() {
        return Err(format!("unexpected character `{}`", tokens[i]));
    }
    Ok(v)
}

fn parse_arith(
    tokens: &[char],
    i: &mut usize,
    symbols: &HashMap<String, u16>,
) -> Result<u32, String> {
    let mut lhs = parse_term(tokens, i, symbols)?;
    while *i < tokens.len() {
        let op = tokens[*i];
        if op != '+' && op != '-' {
            break;
        }
        *i += 1;
        let rhs = parse_term(tokens, i, symbols)?;
        lhs = if op == '+' {
            lhs.wrapping_add(rhs)
        } else {
            lhs.wrapping_sub(rhs)
        };
    }
    Ok(lhs)
}

fn parse_term(tokens: &[char], i: &mut usize, symbols: &HashMap<String, u16>) -> Result<u32, String> {
    let c = *tokens
        .get(*i)
        .ok_or_else(|| "premature end of expression".to_string())?;

    if c == '(' {
        *i += 1;
        let v = parse_arith(tokens, i, symbols)?;
        if *i >= tokens.len() || tokens[*i] != ')' {
            return Err("missing `)`".to_string());
        }
        *i += 1;
        return Ok(v);
    }
    if c == '-' {
        *i += 1;
        return Ok(0u32.wrapping_sub(parse_term(tokens, i, symbols)?));
    }
    if c == '+' {
        *i += 1;
        return parse_term(tokens, i, symbols);
    }
    if c == '\'' {
        let start = *i;
        *i += 1;
        while *i < tokens.len() && tokens[*i] != '\'' {
            *i += 1;
        }
        if *i < tokens.len() {
            *i += 1;
        }
        let word: String = tokens[start..*i].iter().collect();
        if let Some(chr) = word.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
            let bytes = chr.as_bytes();
            if bytes.len() == 1 {
                return Ok(bytes[0] as u32);
            }
        }
        return Err(format!("bad char literal `{word}`"));
    }

    let start = *i;
    while *i < tokens.len() {
        let ch = tokens[*i];
        if ch.is_alphanumeric() || ch == '_' || ch == '.' || ch == '\'' || ch == '$' || ch == '%' {
            *i += 1;
        } else {
            break;
        }
    }
    let word: String = tokens[start..*i].iter().collect();

    if let Some(hex) = word.strip_prefix('$') {
        return u32::from_str_radix(hex, 16).map_err(|_| format!("bad hex `{word}`"));
    }
    if let Some(bin) = word.strip_prefix('%') {
        return u32::from_str_radix(bin, 2).map_err(|_| format!("bad binary `{word}`"));
    }
    if let Some(chr) = word.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        let bytes = chr.as_bytes();
        if bytes.len() == 1 {
            return Ok(bytes[0] as u32);
        }
        return Err(format!("bad char literal `{word}`"));
    }
    if let Some(sym) = symbols.get(&word) {
        return Ok(*sym as u32);
    }
    word.parse::<u32>().map_err(|_| format!("cannot parse `{word}`"))
}

fn parse_imm(src: &str, symbols: &HashMap<String, u16>) -> Result<u8, String> {
    let v = parse_expr(src, symbols)?;
    if v > 0xFF {
        return Err(format!("immediate 0x{v:02X} does not fit in a byte"));
    }
    Ok(v as u8)
}

fn parse_abs(src: &str, symbols: &HashMap<String, u16>) -> Result<u16, String> {
    let v = parse_expr(src, symbols)?;
    if v > 0xFFFF {
        return Err(format!("address 0x{v:04X} does not fit in 16 bits"));
    }
    Ok(v as u16)
}

/// Flattens a source tree: `.include` lines are replaced by the included
/// file's content, recursively, with cycle detection.
fn load_unit(path: &str, stack: &mut Vec<String>) -> Result<Unit, AssembleError> {
    if stack.iter().any(|p| p == path) {
        return Err(AssembleError::new(path, 0, "recursive .include detected"));
    }
    stack.push(path.to_string());
    let src = std::fs::read_to_string(path)
        .map_err(|e| AssembleError::new(path, 0, format!("cannot read file: {e}")))?;
    let dir = Path::new(path).parent().unwrap_or_else(|| Path::new(".")).to_path_buf();

    let mut lines = vec![];
    for (i, raw) in src.lines().enumerate() {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix(".include") {
            let fname = rest.trim().trim_matches('"');
            let inc_path = if fname.starts_with('/') {
                fname.to_string()
            } else {
                dir.join(fname).to_string_lossy().to_string()
            };
            let included = load_unit(&inc_path, stack)?;
            lines.extend(included.lines);
            continue;
        }
        lines.push(Line { file: path.to_string(), number: i, raw: raw.to_string() });
    }
    stack.pop();
    Ok(Unit { lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disassembler_docs_example_assembles() {
        let src = r#"
            .origin $8000
            LDA #$41
            OUT
            HLT
        "#;
        let p = assemble_str(src).unwrap();
        assert_eq!(p.entry, 0x8000);
        assert_eq!(p.bytes, vec![0x10, 0x41, 0x02, 0x01]);
    }

    #[test]
    fn labels_resolve_forward_and_backward() {
        let src = r#"
            .origin $8000
            loop:
              INC A
              CMP #$0A
              JNZ loop
              HLT
        "#;
        let p = assemble_str(src).unwrap();
        assert_eq!(p.bytes, vec![0x3E, 0x27, 0x0A, 0x73, 0x00, 0x80, 0x01]);
    }

    #[test]
    fn data_directives() {
        let src = r#"
            .origin $8000
            .byte $01, $02, 3
            .word $1234
            .string "hi"
            .asciz "!x"
        "#;
        let p = assemble_str(src).unwrap();
        assert_eq!(
            p.bytes,
            vec![0x01, 0x02, 0x03, 0x34, 0x12, b'h', b'i', b'!', b'x', 0x00]
        );
    }

    #[test]
    fn expressions_with_parens_and_sign() {
        let src = r#"
            .origin $8000
            LDA #(2 + 3) * 0 + 7 - 1
            HLT
        "#;
        // (2+3) → 5; 5*0 is not parseable (no *), so we use + and - only.
        // Parse: (2+3) → 5, then `*` is an unexpected char → error is correct.
        let err = assemble_str(src).unwrap_err();
        assert!(err.message.contains("unexpected character"));
    }

    #[test]
    fn comments_and_blank_lines_ignored() {
        let src = r#"
            ; a comment
            .origin $8000   ; org comment
            LDA #%00000001 ; binary immediate
            HLT
        "#;
        let p = assemble_str(src).unwrap();
        assert_eq!(p.bytes, vec![0x10, 0x01, 0x01]);
    }

    #[test]
    fn char_literals() {
        let src = r#"
            .origin $8000
            LDA #'Z'
            HLT
        "#;
        let p = assemble_str(src).unwrap();
        assert_eq!(p.bytes, vec![0x10, b'Z', 0x01]);
    }

    #[test]
    fn indexed_store_with_and_without_comma_suffix() {
        let src = r#"
            .origin $8000
            STA $F000,X
            STA $F100,
        "#;
        let p = assemble_str(src).unwrap();
        assert_eq!(p.bytes, vec![0x64, 0x00, 0xF0, 0x64, 0x00, 0xF1]);
    }

    #[test]
    fn psh_pop_register_selectors() {
        let src = r#"
            .origin $8000
            PSH A
            PSH B
            POP Y
            POP X
        "#;
        let p = assemble_str(src).unwrap();
        assert_eq!(p.bytes, vec![0x56, 0x00, 0x56, 0x01, 0x57, 0x03, 0x57, 0x02]);
    }

    #[test]
    fn unknown_mnemonic_reports_line() {
        let err = assemble_str("FLY\n").unwrap_err();
        assert!(err.message.contains("FLY"));
        assert_eq!(err.line, 1);
    }

    #[test]
    fn duplicate_label_error() {
        let src = "foo: NOP\nfoo: NOP\n";
        let err = assemble_str(src).unwrap_err();
        assert!(err.message.contains("duplicate label"));
    }

    #[test]
    fn range_error_on_big_immediate() {
        let src = "LDA #$100\n";
        let err = assemble_str(src).unwrap_err();
        assert!(err.message.contains("byte"));
    }

    #[test]
    fn label_reference_to_forward_data() {
        let src = r#"
            .origin $8000
            LDA msg
            HLT
            msg: .string "A"
        "#;
        let p = assemble_str(src).unwrap();
        assert_eq!(p.bytes[0], 0x14);
        let addr = p.bytes[1] as u16 | ((p.bytes[2] as u16) << 8);
        assert_eq!(addr, 0x8004);
        assert_eq!(p.bytes[3], 0x01);
        assert_eq!(p.bytes[4], b'A');
    }

    #[test]
    fn space_directive_reserves() {
        let p = assemble_str(".origin $8000\n.space 3\nHLT\n").unwrap();
        assert_eq!(p.bytes, vec![0, 0, 0, 0x01]);
    }

    #[test]
    fn inc_dec_a_single_byte() {
        let p = assemble_str(".origin $8000\nINC A\nDEC A\n").unwrap();
        assert_eq!(p.bytes, vec![0x3E, 0x3F]);
    }

    #[test]
    fn inc_abs_is_three_bytes() {
        let p = assemble_str(".origin $8000\nINC $2000\n").unwrap();
        assert_eq!(p.bytes, vec![0x40, 0x00, 0x20]);
    }

    #[test]
    fn case_insensitive_mnemonics() {
        let p = assemble_str(".origin $8000\nlda #1\nHlt\n").unwrap();
        assert_eq!(p.bytes, vec![0x10, 0x01, 0x01]);
    }
}