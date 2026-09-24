# silicon

Una CPU de 8 bits construida desde cero: conjunto de instrucciones propio,
ensamblador de dos pasadas, desensamblador, máquina virtual, pantalla virtual
de 32×32 y depurador interactivo. **Cero dependencias externas** — solo la
biblioteca estándar de Rust.

## Qué hace

silicon es un ordenador completo e inventado que no copia ninguna arquitectura
existente. Escribes programas en su lenguaje ensamblador, los ensamblas a
binario y los ejecutas en la máquina virtual. Incluye:

- **ISA propia** de 256 opcodes: aritmética con acarreo (ADD/ADC/SUB/SBC),
  lógica, desplazamientos, memoria indexada por X, pila, llamadas y
  retornos, saltos condicionales y un sistema de registro de banderas.
- **Ensamblador de dos pasadas** con etiquetas (adelante y atrás), literales
  hexadecimales (`$`), binarios (`%`), decimales y caracteres (`'A'`),
  expresiones con paréntesis y signo, directivas `.origin`, `.byte`, `.word`,
  `.string`, `.asciz`, `.space` e `.include`.
- **Desensamblador** que vuelve a generar fuente re-ensamblable idéntica
  byte a byte (round-trip).
- **Máquina virtual determinista**: espacio plano de 64 KiB, pila que crece
  hacia abajo, flags Z/C/N, memoria mapeada (pantalla, consola, teclado,
  contador de frames).
- **Pantalla virtual** 32×32 con 16 colores, renderizable como texto o ANSI.
- **Depurador interactivo** en el terminal.

El propósito es pedagógico y de ingeniería: entender lo que ocurre entre el
código fuente y el silicio leyendo cada capa, sin atajos.

## Características

- Sin dependencias: la librería usa únicamente `std`.
- Una única tabla de opcodes como fuente de verdad compartida por el
  ensamblador, la máquina virtual y el desensamblador — no pueden divergir.
- Determinismo total: la misma entrada produce siempre la misma ejecución.
- Errores con fichero y número de línea del ensamblador.
- Round-trip byte-por-byte entre ensamblar y desensamblar.

## Arquitectura

```
       ┌──────────────┐    ┌──────────────┐
       │  assembler   │───▶│     cpu      │───▶ pantalla / consola
       └──────────────┘    │  (64 KiB)    │
              │            └──────┬───────┘
              │                   │
       ┌──────▼─────┐      ┌──────▼──────┐
       │  disasm    │      │    isa      │
       └────────────┘      └─────────────┘
```

- `src/isa.rs` — tabla de 256 opcodes (mnemónico + modo + ciclos).
- `src/assembler.rs` — ensamblador de dos pasadas y directivas.
- `src/cpu.rs` — la máquina virtual completa.
- `src/disasm.rs` — desensamblador, listado y fuente re-ensamblable.
- `src/shadow.rs` — renderizado de la pantalla virtual.
- `src/main.rs` — CLI (`asm`, `run`, `disasm`, `frame`, `debug`).

### El ISA en una ojeada

| Modo | Forma | Tamaño |
|------|-------|--------|
| None | `NOP` | 1 byte |
| Imm  | `LDA #$41` | 2 bytes |
| Abs  | `STA $F800` | 3 bytes |
| AbsX | `STA $F800,X` | 3 bytes |
| Reg  | `PSH A`, `POP X` | 2 bytes |

Registros: `A`, `B`, `X`, `Y`, `PC`, `SP` y banderas `Z`, `C`, `N`.
La pila crece hacia abajo desde `$EFFF`. Región de sistema en `$F000-$F7FF`
(hay `VC_CTRL`, `KEY_PORT`, `TICK_PORT`, `VC_CONSOLE`) y planos de pantalla
en `$F800` (caracteres) y `$FC00` (colores).

## Instalación

```sh
cargo build --release
install -m755 target/release/silicon ~/.local/bin/
```

Requiere Rust (edition 2024). No hay dependencias que descargar.

## Uso

```sh
# Ensamblar
silicon asm programa.s               # genera programa.bin
silicon asm programa.s -o salida.bin

# Ejecutar un ensamblador o binario (el binario arranca en dirección 0)
silicon run programa.s --dump-console
silicon run salida.bin --max-cycles 10000

# Desensamblar
silicon disasm programa.bin --base 0x8000        # listado en pantalla
silicon disasm programa.bin --base 0x8000 -o rt.s

# Pantalla virtual
silicon run programa.s --screen                   # render ANSI en vivo
silicon frame programa.s --frame 20              # fotograma como texto

# Depurar
silicon debug programa.s
```

Ejemplo completo en `examples/diagonal.s`: imprime un mensaje por la consola
y dibuja una diagonal en la pantalla virtual usando direccionamiento indexado.

## Configuración

No hay configuración. Todo el comportamiento se controla por argumentos de
línea de comandos (`silicon help` o `silicon` sin argumentos).

## Testing

```sh
cargo test --release
```

54 pruebas unitarias cubren el ensamblador (literales, etiquetas,
directivas, errores con línea), la CPU (banderas, acarreo, pila,
llamadas/retornos, saltos, modos de direccionamiento, puertos) y el
round-trip del desensamblador. La tabla de opcodes se valida
automáticamente (sin duplicados, todos con coste y nombre).

## Limitaciones

- Programas de hasta 64 KiB (espacio de direcciones completo).
- Aritmética sin decimales ni punto flotante — es una máquina de 8 bits.
- No hay interrupciones ni temporizadores programables.
- Las direcciones bajas (`$0000`-`$7FFF`) requieren que el código cargado
  arranque en ellas; cargar un binario sin punto de entrada usa la
  dirección 0 (pasa `--base`/ensambla con `.origin` si necesitas otra).

## Licencia

MIT — ver [LICENSE](LICENSE).