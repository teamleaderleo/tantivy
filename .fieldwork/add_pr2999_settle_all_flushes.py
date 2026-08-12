from __future__ import annotations

import pathlib
import subprocess
import sys

source_path = pathlib.Path(sys.argv[1])
patch_path = pathlib.Path(sys.argv[2])
test_asset_path = pathlib.Path(sys.argv[3])

subprocess.run(["git", "apply", "--check", str(patch_path)], check=True)
subprocess.run(["git", "apply", str(patch_path)], check=True)

source = source_path.read_text()
needle = "#[cfg(test)]\nmod tests {"
replacement = (
    "#[cfg(test)]\n"
    "mod fieldwork_pr2999_settle_all_flushes;\n\n"
    "#[cfg(test)]\n"
    "mod tests {"
)
if source.count(needle) != 1:
    raise SystemExit("expected exact index_writer test-module marker once")
source_path.write_text(source.replace(needle, replacement, 1))

destination = source_path.parent / "index_writer" / "fieldwork_pr2999_settle_all_flushes.rs"
destination.parent.mkdir(parents=True, exist_ok=True)
destination.write_bytes(test_asset_path.read_bytes())
