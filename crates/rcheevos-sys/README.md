# rcheevos-sys

Bindings to [rcheevos](https://github.com/RetroAchievements/rcheevos), the RetroAchievements client library.

`rcheevos/` is a copy of upstream tag v12.5.0 (commit 1433173220a7eaede6a9ed7a18e94117be1821e0), kept in the tree so the app builds from a checkout without submodules. These upstream files are left out:

- `src/rapi/rc_api_editor.c`: the editor API (achievement uploads, code notes), which the app never calls
- `src/rc_client_types.natvis`, `src/rcheevos/rc_runtime_types.natvis`, `include/module.modulemap`: debugger and Swift module files

To update, copy `include/`, `src/` and `LICENSE` from the new tag, delete the files above again, and change the tag here. `build.rs` compiles every `.c` file under `rcheevos/src` except its `SKIP_FILES` list.
