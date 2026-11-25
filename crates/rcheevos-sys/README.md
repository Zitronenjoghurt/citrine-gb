# rcheevos-sys

Bindings to [rcheevos](https://github.com/RetroAchievements/rcheevos), the RetroAchievements client library.

`rcheevos/` is a copy of upstream tag v12.5.0 (commit 1433173220a7eaede6a9ed7a18e94117be1821e0), kept in the tree so the app builds from a checkout without submodules. These upstream files are left out:

- `src/rapi/rc_api_editor.c`: the editor API (achievement uploads, code notes), which the app never calls
- `src/rc_client_types.natvis`, `src/rcheevos/rc_runtime_types.natvis`, `include/module.modulemap`: debugger and Swift module files

Two local fixes in `src/rhash` stop a crash on a crafted DS ROM header and must be carried over to a new tag unless upstream has them:

- `hash_rom.c`, `rc_hash_nintendo_ds`: the arm9 and arm7 code sizes are added in 64 bits, so two sizes that wrap past 4 GB fail the 16 MB check
- `hash.c`, `rc_file_read_buffered_file`: the read length is compared with the bytes left in 64 bits, so a length over 2 GB can't read past the buffer

To update, copy `include/`, `src/` and `LICENSE` from the new tag, delete the files above again, and change the tag here. `build.rs` compiles every `.c` file under `rcheevos/src` except its `SKIP_FILES` list.
