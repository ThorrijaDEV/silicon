//! silicon — a scratch-built 8-bit computer: assembler, virtual machine,
//! disassembler, virtual screen and debugger. Zero external dependencies.

use std::io::{BufRead, Write};
use std::process::ExitCode;

use silicon::assembler::{assemble as asm_file, AssembleError};
use silicon::cpu::Cpu;
use silicon::disasm;
use silicon::shadow;
use silicon::Program;

const MAX_CYCLES: u64 = 500_000;
/// Cycles emulated per animated frame (tunes animation speed).
const CYCLES_PER_FRAME: u64 = 4096;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    r#"silicon — 8-bit computer built from scratch

USAGE:
  silicon asm     <file.s> [-o out.bin]
  silicon disasm  <file.bin> [-o out.asm] [--base HEX]
  silicon run     <file.s|file.bin> [--screen] [--max-cycles N] [--dump-console]
  silicon debug   <file.s|file.bin>
  silicon frame   <file.s|file.bin> [--frame N]

  --screen renders the 32x32 virtual display live (16 colours).
  --frame N  prints the display as plain text after N frames.
  --dump-console prints the OUT console buffer after halting.
"#
    .to_string()
}

fn run(args: &[String]) -> Result<(), String> {
    let Some(cmd) = args.first() else {
        print!("{}", usage());
        return Ok(());
    };
    match cmd.as_str() {
        "asm" => cmd_asm(&args[1..]),
        "disasm" => cmd_disasm(&args[1..]),
        "run" => cmd_run(&args[1..]),
        "debug" => cmd_debug(&args[1..]),
        "frame" => cmd_frame(&args[1..]),
        "help" | "--help" | "-h" => {
            print!("{}", usage());
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!("silicon {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(format!("unknown command `{other}` — try `silicon help`")),
    }
}

fn load_program(path: &str) -> Result<Program, String> {
    let lower = path.to_lowercase();
    if lower.ends_with(".s") || lower.ends_with(".asm") {
        asm_file(path).map_err(|e: AssembleError| e.to_string())
    } else {
        let bytes = std::fs::read(path).map_err(|e| format!("cannot read `{path}`: {e}"))?;
        Ok(Program { bytes, entry: 0, symbols: Default::default() })
    }
}

fn make_cpu(prog: &Program) -> Cpu {
    let mut cpu = Cpu::new();
    cpu.load(prog.entry, &prog.bytes);
    cpu
}

/// Extracts `--flag <value>` from argv, returning (value, remaining args).
fn pop_flag(args: &[String], flag: &str) -> (Option<String>, Vec<String>) {
    let mut val = None;
    let mut rest = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == flag {
            if let Some(v) = it.next() {
                val = Some(v.clone());
            }
        } else {
            rest.push(a.clone());
        }
    }
    (val, rest)
}

fn cmd_asm(args: &[String]) -> Result<(), String> {
    let (out, rest) = pop_flag(args, "-o");
    let path = rest.first().ok_or("usage: silicon asm <file.s> [-o out.bin]")?;
    let prog = asm_file(path).map_err(|e: AssembleError| e.to_string())?;
    let dest = out.unwrap_or_else(|| path.trim_end_matches(".s").to_string() + ".bin");
    std::fs::write(&dest, &prog.bytes).map_err(|e| format!("cannot write `{dest}`: {e}"))?;
    println!(
        "assembled {} -> {} ({} bytes, entry ${:04X})",
        path,
        dest,
        prog.bytes.len(),
        prog.entry
    );
    Ok(())
}

fn cmd_disasm(args: &[String]) -> Result<(), String> {
    let (out, rest) = pop_flag(args, "-o");
    let (base, rest) = pop_flag(&rest, "--base");
    let path = rest.first().ok_or("usage: silicon disasm <file.bin> [-o out.asm]")?;
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read `{path}`: {e}"))?;
    let base = match base {
        Some(b) => u16::from_str_radix(b.trim_start_matches("0x").trim_start_matches('$'), 16)
            .map_err(|_| format!("bad --base value `{b}`"))?,
        None => 0,
    };
    if let Some(dest) = out {
        let src = disasm::source(&bytes, base);
        std::fs::write(&dest, src).map_err(|e| format!("cannot write `{dest}`: {e}"))?;
        println!(
            "disassembled {} -> {} ({} instructions)",
            path,
            dest,
            disasm::decode(&bytes, base).len()
        );
    } else {
        print!("{}", disasm::listing(&bytes, base));
    }
    Ok(())
}

fn cmd_run(args: &[String]) -> Result<(), String> {
    let (screen, rest) = pop_flag(args, "--screen");
    let (maxc, rest) = pop_flag(&rest, "--max-cycles");
    let (dump, rest) = pop_flag(&rest, "--dump-console");
    let path = rest.first().ok_or("usage: silicon run <file.s|file.bin>")?;
    let prog = load_program(path)?;
    let budget = maxc
        .map(|m| m.parse::<u64>().map_err(|_| "bad --max-cycles value".to_string()))
        .transpose()?
        .unwrap_or(MAX_CYCLES);

    let mut cpu = make_cpu(&prog);

    if screen.is_some() {
        run_screen(&mut cpu, budget);
    } else {
        cpu.run_budget(budget);
        if !cpu.halted {
            return Err(format!(
                "program exceeded its cycle budget ({budget} cycles) — add a HLT or raise --max-cycles"
            ));
        }
        if dump.is_some() && !cpu.console.is_empty() {
            use std::io::Write;
            print!("{}", String::from_utf8_lossy(&cpu.console));
            std::io::stdout().flush().ok();
        }
        println!(
            "halted: pc=${:04X} a=${:02X} b=${:02X} x=${:02X} y=${:02X} sp=${:04X} cycles={} frames={}",
            cpu.pc, cpu.a, cpu.b, cpu.x, cpu.y, cpu.sp, cpu.cycles, cpu.frames
        );
        if !cpu.console.is_empty() && dump.is_none() {
            println!("console: {}", String::from_utf8_lossy(&cpu.console));
        }
    }
    Ok(())
}

/// Runs with the virtual display rendered live. Endless demos (driven by the
/// TICK port) run for the whole budget and exit when it is spent; finite
/// programs stop at HLT.
fn run_screen(cpu: &mut Cpu, budget: u64) {
    use std::io::IsTerminal;
    let tty = std::io::stdout().is_terminal();
    let start = cpu.cycles;
    while cpu.cycles - start < budget {
        let frame = (start + budget).min(cpu.cycles + CYCLES_PER_FRAME);
        cpu.run_budget(frame - cpu.cycles);
        let plane = cpu.screen();
        if tty {
            print!("{}", shadow::render_ansi(&plane));
        } else {
            for line in shadow::frame_text(&plane) {
                println!("{line}");
            }
        }
        let _ = std::io::stdout().flush();
        if cpu.halted {
            // Finite program: give its final frame a moment before closing.
            std::thread::sleep(std::time::Duration::from_millis(400));
            break;
        }
    }
}

fn cmd_debug(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("usage: silicon debug <file.s|file.bin>")?;
    let prog = load_program(path)?;
    let mut cpu = make_cpu(&prog);
    let mut breakpoints: Vec<u16> = vec![];
    let stdin = std::io::stdin();
    let mut input = stdin.lock();

    println!("silicon debugger — type `help` for commands");
    loop {
        let line = read_line(&mut input)?;
        let mut parts = line.split_whitespace();
        let Some(cmd) = parts.next() else {
            continue;
        };
        match cmd {
            "q" | "quit" | "exit" => break,
            "h" | "help" => print_help(),
            "r" | "reg" => print_regs(&cpu),
            "s" | "step" => {
                let n = parts.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
                for _ in 0..n.min(10_000) {
                    if cpu.halted {
                        break;
                    }
                    cpu.step();
                }
                println!("-- pc=${:04X} halted={}", cpu.pc, cpu.halted);
            }
            "run" | "c" | "continue" => {
                while !cpu.halted {
                    cpu.step();
                    if breakpoints.contains(&cpu.pc) {
                        println!("-- breakpoint hit at ${:04X}", cpu.pc);
                        break;
                    }
                }
                if cpu.halted {
                    println!("-- halted");
                }
            }
            "b" | "break" => {
                let count = parts
                    .filter_map(parse_addr)
                    .inspect(|a| {
                        breakpoints.push(*a);
                        println!("breakpoint set at ${a:04X}");
                    })
                    .count();
                if count == 0 {
                    println!("usage: b <addr>...");
                }
            }
            "nb" => {
                if let Some(a) = parts.next().and_then(parse_addr) {
                    breakpoints.retain(|&b| b != a);
                    println!("breakpoint cleared at ${a:04X}");
                }
            }
            "lb" => {
                if breakpoints.is_empty() {
                    println!("no breakpoints");
                } else {
                    for b in &breakpoints {
                        println!("${b:04X}");
                    }
                }
            }
            "d" | "disasm" => {
                let n = parts.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(6);
                let start = parts.next().and_then(parse_addr).unwrap_or(cpu.pc);
                for ins in disasm::decode(&bytes_at(&cpu, start), start).into_iter().take(n) {
                    let marker = if ins.addr == cpu.pc { ">" } else { " " };
                    println!("{marker}${:04X}  {:<11} {}", ins.addr, hexstr(&ins.bytes), ins.text);
                }
            }
            "m" | "mem" => {
                let start = parts.next().and_then(parse_addr).unwrap_or(cpu.pc);
                let len = parts.next().and_then(|s| s.parse::<usize>().ok()).unwrap_or(16);
                dump_mem(&cpu, start, len);
            }
            "set" => {
                let addr = parts.next().and_then(parse_addr);
                let val = parts.next();
                match (addr, val) {
                    (Some(a), Some(v)) => {
                        let v = parse_u8(v).ok_or_else(|| format!("bad byte `{v}`"))?;
                        cpu.write(a, v);
                        println!("${a:04X} = ${v:02X}");
                    }
                    _ => println!("usage: set <addr> <byte>"),
                }
            }
            "p" | "pc" => {
                if let Some(a) = parts.next().and_then(parse_addr) {
                    cpu.pc = a;
                    println!("pc = ${a:04X}");
                }
            }
            other => println!("unknown command `{other}` — try `help`"),
        }
    }
    Ok(())
}

fn bytes_at(cpu: &Cpu, start: u16) -> Vec<u8> {
    let end = (start as usize + 128).min(cpu.mem.len());
    cpu.mem[start as usize..end].to_vec()
}

fn hexstr(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

fn parse_u8(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('$') {
        u8::from_str_radix(h, 16).ok()
    } else {
        s.parse::<u8>().ok()
    }
}

fn parse_addr(s: &str) -> Option<u16> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('$').or_else(|| s.strip_prefix("0x")) {
        u16::from_str_radix(h, 16).ok()
    } else {
        s.parse::<u16>().ok()
    }
}

fn print_help() {
    println!(
        r#"commands:
  r / reg          show registers and flags
  s N              step N instructions
  b <addr>...      set breakpoint(s)
  nb <addr>        clear breakpoint
  lb               list breakpoints
  run / c          continue until HLT or breakpoint
  d [n] [addr]     disassemble n instructions at addr (default pc)
  m [addr] [len]   dump memory as bytes
  set <addr> <val> poke a byte
  p <addr>         point the pc at addr
  q                quit
addresses accept $hex, 0x hex or decimal"#
    );
}

fn print_regs(cpu: &Cpu) {
    println!(
        "a=${:02X} b=${:02X} x=${:02X} y=${:02X} sp=${:04X} pc=${:04X} cycles={} frames={} halted={}",
        cpu.a, cpu.b, cpu.x, cpu.y, cpu.sp, cpu.pc, cpu.cycles, cpu.frames, cpu.halted
    );
    println!(
        "flags: Z={} C={} N={}",
        cpu.flags.z as u8,
        cpu.flags.c as u8,
        cpu.flags.n as u8
    );
}

fn dump_mem(cpu: &Cpu, addr: u16, len: usize) {
    for chunk in (0..len).step_by(8) {
        let base = addr as usize + chunk;
        let mut row = format!("${:04X}: ", base as u16);
        for k in chunk..(chunk + 8).min(len) {
            row.push_str(&format!("{:02X} ", cpu.mem[(base + k) & 0xFFFF]));
        }
        println!("{}", row.trim_end());
    }
}

fn read_line<R: BufRead>(r: &mut R) -> Result<String, String> {
    print!("silicon> ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| format!("input error: {e}"))?;
    Ok(line.trim().to_string())
}

fn cmd_frame(args: &[String]) -> Result<(), String> {
    let (frame, rest) = pop_flag(args, "--frame");
    let path = rest.first().ok_or("usage: silicon frame <file.s|file.bin> [--frame N]")?;
    let prog = load_program(path)?;
    let mut cpu = make_cpu(&prog);
    let target = frame
        .map(|f| f.parse::<u64>().map_err(|_| "bad --frame value".to_string()))
        .transpose()?
        .unwrap_or(0);
    while cpu.frames < target && !cpu.halted {
        cpu.step();
    }
    for line in shadow::frame_text(&cpu.screen()) {
        println!("{line}");
    }
    Ok(())
}