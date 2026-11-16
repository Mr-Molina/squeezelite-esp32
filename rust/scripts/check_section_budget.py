#!/usr/bin/env python3
"""Check the memory-section budgets of the louder-esp32-app firmware ELF.

Reads the ELF section headers directly (no binutils needed) and fails when a
budget is exceeded. Budgets and their sources:

  IRAM (.vectors + .rwtext + .rwtext.wifi) <= 131072 B   Invariant 48
  .dram2_uninit                            <=  98768 B   Invariant 48
  .stack                                   >=  28000 B   Invariant 48

Usage: check_section_budget.py <elf> [--min-stack N] [--max-iram N] [--max-dram2 N]
"""

import argparse
import struct
import sys

IRAM_SECTIONS = (".vectors", ".rwtext", ".rwtext.wifi")


def read_sections(path):
    """Return {section name: size in bytes} for a 32-bit little-endian ELF."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] != b"\x7fELF":
        raise ValueError(f"{path}: not an ELF file")
    if data[4] != 1 or data[5] != 1:
        raise ValueError(f"{path}: expected a 32-bit little-endian ELF")
    shoff, = struct.unpack_from("<I", data, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", data, 0x2E)
    if shoff == 0 or shnum == 0:
        raise ValueError(f"{path}: no section headers (stripped image?)")

    def header(index):
        # sh_name, sh_type, sh_flags, sh_addr, sh_offset, sh_size
        return struct.unpack_from("<IIIIII", data, shoff + index * shentsize)

    _, _, _, _, str_off, str_size = header(shstrndx)
    strtab = data[str_off:str_off + str_size]

    sizes = {}
    for i in range(shnum):
        name_off, _, _, _, _, size = header(i)
        end = strtab.index(b"\0", name_off)
        sizes[strtab[name_off:end].decode("ascii", "replace")] = size
    return sizes


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("elf")
    parser.add_argument("--max-iram", type=int, default=131072)
    parser.add_argument("--max-dram2", type=int, default=98768)
    parser.add_argument("--min-stack", type=int, default=28000)
    args = parser.parse_args(argv)

    try:
        sizes = read_sections(args.elf)
    except (OSError, ValueError, struct.error) as err:
        print(f"error: {err}", file=sys.stderr)
        return 2

    required = IRAM_SECTIONS + (".dram2_uninit", ".stack")
    missing = [name for name in required if name not in sizes]
    if missing:
        # A renamed section would otherwise read as 0 bytes and pass silently.
        print(f"error: sections not found in ELF: {', '.join(missing)}", file=sys.stderr)
        return 2

    iram = sum(sizes[name] for name in IRAM_SECTIONS)
    checks = [
        ("IRAM (" + " + ".join(IRAM_SECTIONS) + ")", iram, "<=", args.max_iram),
        (".dram2_uninit", sizes[".dram2_uninit"], "<=", args.max_dram2),
        (".stack", sizes[".stack"], ">=", args.min_stack),
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
