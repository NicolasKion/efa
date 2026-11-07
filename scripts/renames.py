"""Write pyfa's renamed-item table (service/conversions) as src/port/renames.tsv.

Usage: python3 -I scripts/renames.py /path/to/pyfa src/port/renames.tsv

pyfa merges the packs in module name order, later ones winning; this does the same.
"""

import importlib.util
import os
import pkgutil
import sys


def main():
    pyfa, out = sys.argv[1], sys.argv[2]
    base = os.path.join(pyfa, "service", "conversions")
    merged = {}
    for module in sorted(pkgutil.iter_modules([base]), key=lambda m: m.name):
        spec = importlib.util.spec_from_file_location(
            module.name, os.path.join(base, module.name + ".py")
        )
        pack = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(pack)
        merged.update(pack.CONVERSIONS)
    with open(out, "w", encoding="utf-8") as f:
        f.write("# Old item name, tab, current name. From pyfa's service/conversions.\n")
        for old, new in merged.items():
            old, new = old.strip(), new.strip()
            if old and new and old != new:
                f.write(f"{old}\t{new}\n")


if __name__ == "__main__":
    main()
