#!/usr/bin/env python3
"""Check the memory budgets of the louder-esp32-app firmware ELF.

Reads the ELF section headers and symbol table directly (no binutils needed)
and fails when a budget is exceeded.

Hard outer limits (Invariant 48; the linker or the radio stack fails beyond
them):

  IRAM (.vectors + .rwtext + .rwtext.wifi) <= 131072 B
  .dram2_uninit                            <=  98768 B
  .stack                                   >=  28000 B
  internal heap (both regions)             >=  65536 B

The defaults are tighter: the values measured when work order F3 (memory
re-budget) closed, with 256 B of slack, so that reclaimed memory cannot be
spent silently. A feature branch that needs its allowance (design spec
section 7) moves a default by at most that allowance in the same pull
request. No limit may go beyond the hard limits above.

  --max-iram   119976   (measured 119720)
  --max-dram2   89048   (measured  88792)
  --min-stack   41256   (measured  41512)
  --min-heap    65536   (measured  65536; this is the hard limit itself)

The internal heap is the sum of the two statics the firmware registers as
`MemoryCapability::Internal` regions: `bring_up::HEAP` (.bss) and
`STATIC_BT_HEAP` (.dram2_uninit).

Usage: check_section_budget.py <elf> [--min-stack N] [--max-iram N]
                                     [--max-dram2 N] [--min-heap N]
"""

import argparse
import struct
import sys

IRAM_SECTIONS = (".vectors", ".rwtext", ".rwtext.wifi")

HARD_MAX_IRAM = 131072
HARD_MAX_DRAM2 = 98768
HARD_MIN_STACK = 28000
HARD_MIN_HEAP = 65536

# Endings of the (v0-mangled) names of the two internal heap statics in
# `louder-esp32-app/src/main.rs`.
HEAP_SYMBOL_SUFFIXES = ("8bring_up4HEAP", "14STATIC_BT_HEAP")
SHT_SYMTAB = 2


def _load(path):
    with open(path, "rb") as f:
        data = f.read()
    if len(data) < 0x34:
        raise ValueError(f"{path}: truncated ELF header")
    if data[:4] != b"\x7fELF":
        raise ValueError(f"{path}: not an ELF file")
    if data[4] != 1 or data[5] != 1:
        raise ValueError(f"{path}: expected a 32-bit little-endian ELF")
    shoff, = struct.unpack_from("<I", data, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", data, 0x2E)
    if shoff == 0 or shnum == 0:
        raise ValueError(f"{path}: no section headers (stripped image?)")
    return data, shoff, shentsize, shnum, shstrndx


def _cstr(table, offset):
    end = table.index(b"\0", offset)
    return table[offset:end].decode("ascii", "replace")


def read_sections(path):
    """Return {section name: size in bytes} for a 32-bit little-endian ELF."""
    data, shoff, shentsize, shnum, shstrndx = _load(path)

    def header(index):
        # sh_name, sh_type, sh_flags, sh_addr, sh_offset, sh_size
        return struct.unpack_from("<IIIIII", data, shoff + index * shentsize)

    _, _, _, _, str_off, str_size = header(shstrndx)
    strtab = data[str_off:str_off + str_size]

    sizes = {}
    for i in range(shnum):
        name_off, _, _, _, _, size = header(i)
        name = _cstr(strtab, name_off)
        sizes[name] = sizes.get(name, 0) + size
    return sizes


def is_heap_symbol(name):
    """True for the firmware's two internal heap region statics."""
    return "louder_esp32_app" in name and name.endswith(HEAP_SYMBOL_SUFFIXES)


def read_heap_symbols(path):
    """Return {symbol name: size} of the internal heap statics in the ELF."""
    data, shoff, shentsize, shnum, _ = _load(path)

    def header(index):
        # sh_name, sh_type, sh_flags, sh_addr, sh_offset, sh_size, sh_link
        return struct.unpack_from("<IIIIIII", data, shoff + index * shentsize)

    found = {}
    for i in range(shnum):
        _, sh_type, _, _, off, size, link = header(i)
        if sh_type != SHT_SYMTAB:
            continue
        _, _, _, _, str_off, str_size, _ = header(link)
        strtab = data[str_off:str_off + str_size]
        for sym in range(size // 16):
            # Elf32_Sym is 16 bytes: st_name, st_value, st_size, ...
            st_name, _, st_size = struct.unpack_from("<III", data, off + sym * 16)
            name = _cstr(strtab, st_name)
            if is_heap_symbol(name):
                found[name] = st_size
    return found


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("elf")
    parser.add_argument("--max-iram", type=int, default=119976)
    parser.add_argument("--max-dram2", type=int, default=89048)
    parser.add_argument("--min-stack", type=int, default=41256)
    parser.add_argument("--min-heap", type=int, default=65536)
    args = parser.parse_args(argv)

    beyond = [
        flag
        for flag, bad in (
            ("--max-iram", args.max_iram > HARD_MAX_IRAM),
            ("--max-dram2", args.max_dram2 > HARD_MAX_DRAM2),
            ("--min-stack", args.min_stack < HARD_MIN_STACK),
            ("--min-heap", args.min_heap < HARD_MIN_HEAP),
        )
        if bad
    ]
    if beyond:
        print(
            "error: beyond the Invariant 48 hard limit: " + ", ".join(beyond),
            file=sys.stderr,
        )
        return 2

    try:
        sizes = read_sections(args.elf)
        heap = read_heap_symbols(args.elf)
    except (OSError, ValueError, struct.error) as err:
        print(f"error: {err}", file=sys.stderr)
        return 2

    required = IRAM_SECTIONS + (".dram2_uninit", ".stack")
    missing = [name for name in required if name not in sizes]
    if missing:
        # A renamed section would otherwise read as 0 bytes and pass silently.
        print(f"error: sections not found in ELF: {', '.join(missing)}", file=sys.stderr)
        return 2
    if len(heap) != len(HEAP_SYMBOL_SUFFIXES):
        # A renamed heap static would otherwise count as 0 bytes.
        print(
            "error: expected the internal heap statics "
            + ", ".join(HEAP_SYMBOL_SUFFIXES)
            + f"; found {sorted(heap)}",
            file=sys.stderr,
        )
        return 2

    iram = sum(sizes[name] for name in IRAM_SECTIONS)
    checks = [
        ("IRAM (" + " + ".join(IRAM_SECTIONS) + ")", iram, "<=", args.max_iram),
        (".dram2_uninit", sizes[".dram2_uninit"], "<=", args.max_dram2),
        (".stack", sizes[".stack"], ">=", args.min_stack),
        ("internal heap (2 regions)", sum(heap.values()), ">=", args.min_heap),
    ]

    failed = False
    for label, value, op, limit in checks:
        ok = value <= limit if op == "<=" else value >= limit
        headroom = limit - value if op == "<=" else value - limit
        print(f"{'ok  ' if ok else 'FAIL'} {label}: {value} B ({op} {limit} B, headroom {headroom} B)")
        failed |= not ok
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
