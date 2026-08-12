from __future__ import annotations

import pathlib
import sys

source_path = pathlib.Path(sys.argv[1])
asset_path = pathlib.Path(sys.argv[2])
source = source_path.read_text()
needle = "#[cfg(test)]\nmod tests {"
replacement = (
    "#[cfg(test)]\n"
    "mod fieldwork_delete_all_stale_delete;\n\n"
    "#[cfg(test)]\n"
    "mod tests {"
)
if source.count(needle) != 1:
    raise SystemExit("expected exact index_writer test-module marker once")
source_path.write_text(source.replace(needle, replacement, 1))

destination = source_path.parent / "index_writer" / "fieldwork_delete_all_stale_delete.rs"
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_bytes(asset_path.read_bytes())
