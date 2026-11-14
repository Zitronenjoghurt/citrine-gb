# DS JIT fundamentals

How fast DS and ARM JITs are built, and where the citrine-gb DS JIT differs.

## Why this exists

On an M6 Mac our DS JIT runs MK race CPUs at 4.9 ms a frame against 4.4 ms with the JIT off. In HG, compiled code is 33.6% of the frame and the dispatcher another 10%. melonDS on the same Mac runs its JIT 1.3x faster than its interpreter in MK (3.24 ms against 4.18) and 1.8x in HG (1.84 against 3.25), all on one thread. This file collects how melonDS, Dynarmic, DeSmuME and Dolphin build their JITs, what Cranelift can and cannot do as a backend, and a ranked list of where ours pays more than theirs.

## Sources

| Source | Version read | How |
| --- | --- | --- |
| melonDS | b86390e4 (2025-11-18), the `crates/melonds-sys/melonDS` submodule our benches build | source |
| Dynarmic | lioncash/dynarmic a41c380 (2024-03-03) | source and `docs/` |
| DeSmuME | TASEmulators/desmume 559bf21 (2026-11-06), `desmume/src/arm_jit.cpp` | source |
| Dolphin | dolphin-emu.org articles and the dolphin-emu/dolphin tree | web, links inline |
| Cranelift | cranelift-codegen 0.137.1 (the version in our `Cargo.lock`), crates.io source, Bytecode Alliance docs and posts | source and web |
| citrine-gb | 169b10e5 | source |

melonDS paths below are relative to `crates/melonds-sys/melonDS/src/`. Dynarmic paths are relative to its repository root. Our paths are relative to `crates/citrine-gb/src/`.

## The constraint that decides most of this

Our JIT has to match the interpreter exactly. The `jit-lockstep` feature records the core state and the CPU clock at every block end and compares both with an interpreter run (`nds/machine/lockstep.rs:101-130`), and every JIT change has to pass it. melonDS has no such rule. Its compiled code takes its cycle counts from one interpreted run of the block at compile time, lets a block run past the end of the time slice, and sees IRQs only between blocks (details below). Several of melonDS's main savings come straight from that freedom. The comparison table marks which ideas survive exact timing and which need it relaxed.

Since ticket 83ic, DS timing is no longer cycle-exact at the slice end. A DS CPU stops for its slice only at an end point: right after a pipeline refill (a branch, an exception entry or an IRQ) or where the next instruction starts a 128-byte line (`Core::ends`, `gba/cpu.rs`). It may run past the slice end until it reaches one. The interpreter, both native back ends and Cranelift stop at the same points, so lockstep still holds. GBA keeps exact timing.

## melonDS

melonDS has one JIT front end (`ARMJIT.cpp`) and two hand-written back ends, `ARMJIT_x64/` and `ARMJIT_A64/`. The Mac runs the A64 one. The defaults are 32-instruction blocks with literal and branch optimisations on and fastmem on where supported (`Args.h:59-70`), and our wrapper keeps them (`crates/melonds-sys/src/shim.cpp:499-533`).

### Fastmem is off on the Mac

`ARMJIT_Memory::IsFastMemSupported` returns false for every `__APPLE__` build (`ARMJIT_Memory.cpp:744-747`), and the constructor ANDs the setting with it (`ARMJIT.cpp:478`). So the melonDS numbers in "Why this exists" come from a JIT that calls a C++ function for every load and store whose address is not a compile-time constant. Fastmem does not explain the gap on the Mac.

### Idle loops count in its JIT ratio

melonDS skips idle loops only in its JIT path (`ARM.cpp:639-655`); its interpreter loop has no such check (`ARM.cpp:701-716`). Part of its 1.3x and 1.8x is idle skipping, not faster code. A melonDS build with `IsIdleLoop` returning false would split the two.

### Block size and what ends a block

A block holds at most 32 instructions (`ARMJIT.cpp:475`, `ARMJIT.cpp:822`). It ends at an instruction that writes the PC (`ARM_InstrInfo.cpp:414`, `ARM_InstrInfo.h:271-274`), at the CP15 halt and cache operations (`ARM_InstrInfo.cpp:440-447`), and when an enabled IRQ is already pending while scanning (`ARMJIT.cpp:822`). With branch optimisations on, three kinds of branch do not end it (`ARMJIT.cpp:743-814`):

- a static B or BL that was taken during the scan: the block continues at the target and remembers LR;
- a BX LR whose LR was set earlier in the same block: the return is followed too (`ARMJIT.cpp:322-327`, `343-348`);
- a conditional branch: the block continues on the side the scan took, and the compiled code exits if the other side is taken at run time (`ARMJIT_A64/ARMJIT_Compiler.cpp:657-674`).

A block is entered only at its first instruction. A jump into the middle of a block compiles a new block that starts there; the block map is keyed by start address (`ARMJIT.cpp:544-566`, `927-930`).

Why it is fast: a typical Thumb function body with a few calls and a loop becomes one or two blocks, so most guest instructions run without a trip through the dispatcher.

### Compiling

Compiling is synchronous and happens the first time a block start is reached (`ARM.cpp:632-637`). The scanner runs each instruction through the interpreter while it decodes it (`ARMJIT.cpp:676-698`). That first run supplies the branch directions above and the real code cycles, data cycles and data region of every instruction (`ARMJIT.cpp:648`, `700-701`). The emitter is one pass over the instruction list with no IR (`ARMJIT_A64/ARMJIT_Compiler.cpp:676-829`). A block that is invalidated but whose code comes back unchanged (same instruction hash, literal hash and address ranges) is reused instead of recompiled (`ARMJIT.cpp:844-908`), which helps overlays that reload the same code.

### Cycle counting

Cycles are counted per block. For each instruction the emitter computes its cost at compile time from the first run's code and data timings and adds it to `ConstantCycles` (`ARMJIT_A64/ARMJIT_Compiler.cpp:844-966`). One `ADD` puts the sum into the pinned cycle register at the block exit (`ARMJIT_A64/ARMJIT_Compiler.cpp:820-824`). Only conditional ARM instructions add their own cycles at run time, because their cost depends on the condition. Nothing inside a block checks a budget. The run loop compares the timestamp with the target after each block (`ARM.cpp:617`), so a block may overshoot the slice end by up to 32 instructions. The slice is 64 system cycles, the same as ours (`NDS.cpp:52`, `NDS.cpp:813-836`).

How it stays exact enough: it does not try to be exact. A load from main RAM is charged what the first run paid, whatever the address is later, and an I/O access is charged by its first region.

### Registers

`ARMJIT_RegisterCache.h` allocates guest registers per block over the whole instruction list. Before each instruction it unloads registers that are never used again, evicts the least-used register when it runs out, loads a register on first use (skipping the load when the instruction only writes it) and writes back only dirty registers (`ARMJIT_RegisterCache.h:111-197`). A64 has 15 host registers for guest registers: seven callee-saved (W19-W25) and eight caller-saved (W8-W15) (`ARMJIT_A64/ARMJIT_Compiler.cpp:49-57`). Four host registers stay pinned for the life of a block: the CPU pointer, the cycle counter, the CPSR and the fastmem base (`ARMJIT_A64/ARMJIT_Compiler.h:36-39`). Guest registers do not stay in host registers across blocks; every block exit writes back and the next block reloads.

Flags use liveness. `FloodFillSetFlags` walks backwards and marks only the flags that a later instruction reads, plus all flags at the block end (`ARMJIT.cpp:247-263`, `818-821`, `896`). An ALU instruction whose flags nobody reads is emitted without its flag update (`ARMJIT_A64/ARMJIT_ALU.cpp:261-262`). Flags land in the pinned CPSR register with `CSET` and `BFI` only for the marked bits (`ARMJIT_A64/ARMJIT_ALU.cpp:162-190`). A condition is one `TBZ` or `TBNZ` on the CPSR register for EQ to VC, or five instructions with a lookup table for HI to LE (`ARMJIT_A64/ARMJIT_Compiler.cpp:508-528`).

### Dispatcher and block linking

melonDS has no block linking. Every block ends with a tail call to `ARM_Ret` (`ARMJIT_A64/ARMJIT_Compiler.cpp:824`), which stores the cycle and CPSR registers and restores the twelve callee-saved registers (`ARMJIT_A64/ARMJIT_Linkage.S:47-66`). The C++ run loop (`ARM.cpp:617-656`) then:

1. checks that the PC is inside the cached executable region, or looks the region up;
2. reads one `u64` from a flat table that has one entry per halfword of code memory, holding the guest address with the CPU number and the offset of the compiled code (`ARMJIT.cpp:932-934`, `1065-1071`), and compares it;
3. calls `ARM_Dispatch`, which saves the callee-saved registers, loads the two pinned registers and branches to the block (`ARMJIT_A64/ARMJIT_Linkage.S:28-44`).

The entry is not validated: a stale block is removed when its code is written (see invalidation), so a table hit is always good.

Why it is fast: the whole round trip is a few dozen instructions, and with blocks of up to 32 instructions that follow branches, it runs once for many guest instructions.

### Memory

Without fastmem (the Mac), a load or store calls `SlowRead9`, `SlowWrite9` or the ARM7 versions (`ARMJIT_A64/ARMJIT_LoadStore.cpp:259-330`). `SlowRead9` checks ITCM, then DTCM, then calls `NDS::ARM9Read32` (`ARMJIT.cpp:96-117`). Around the call, `PushRegs` saves only the guest registers that live in caller-saved host registers (`ARMJIT_A64/ARMJIT_Compiler.cpp:164-207`). When the address is a constant known at compile time, the call goes straight to the device handler, for example `GPU3D_Write32` (`ARMJIT_Memory.cpp:1501-1530`, used at `ARMJIT_A64/ARMJIT_LoadStore.cpp:227-258`). LDM and STM make one call for the whole register list (`ARMJIT.cpp:181-192`).

With fastmem (Linux and Windows), each CPU has a 4 GiB host reservation (`ARMJIT_Memory.cpp:108`, `926-927`). Pages are mapped lazily: the first fault on an unmapped page maps that mirror (`ARMJIT_Memory.cpp:632-660`, `821-842`). The access itself is two instructions, an align and a load or store against the base register (`ARMJIT_A64/ARMJIT_LoadStore.cpp:192-224`). A fault on an address that cannot be mapped, such as I/O, rewrites that one access in place into a call to a slow stub made for that register, and resumes there (`ARMJIT_A64/ARMJIT_LoadStore.cpp:36-62`, stubs at `ARMJIT_A64/ARMJIT_Compiler.cpp:349-441`). If the first run already saw a non-RAM region, the slow call is emitted directly (`ARMJIT_A64/ARMJIT_LoadStore.cpp:188-192`). Pages holding compiled code are mapped read-only, so the first store into one faults and its site becomes a slow call that checks for code (`ARMJIT_Memory.cpp:499-531`).

### Literals and constants

A PC-relative load reads its literal at compile time and becomes a move of an immediate (`ARMJIT_A64/ARMJIT_LoadStore.cpp:64-105`, `115-121`). Two things keep that correct: the block stores a hash of all its literal values, and an `InvalidLiterals` list collects literal addresses that a later write touched, after which they load at run time again (`ARMJIT.cpp:703-728`, `824-845`, `977-991`). The register cache also remembers which registers hold such constants (`PutLiteral`), so a later `[rN, #imm]` access has a compile-time address and can call the exact device handler (`ARMJIT_A64/ARMJIT_LoadStore.cpp:150-154`, `188-190`, `227-229`).

### Idle loops

`IsIdleLoop` takes Dolphin's rule (`ARMJIT.cpp:353-382`): a backward conditional branch to a target inside the block, where the loop body has no memory write, no MSR or MCR, no other branch, and no register that one iteration writes after reading it from the previous one. Such a branch is marked (`ARMJIT.cpp:770-779`). When it is taken at run time the code sets `IdleLoop` (`ARMJIT_A64/ARMJIT_Compiler.cpp:657-663`), and the run loop jumps the CPU's timestamp to the slice target (`ARM.cpp:645-653`). This catches polling loops such as "load a flag, compare, branch back", which is how DS games wait for VBlank or for the other CPU.

### IRQs and I/O inside a block

A block never leaves early for an IRQ or an I/O access. `Halted`, `IRQ` and `IdleLoop` are bytes of one `u32`, `StopExecution` (`ARM.h:156-165`), and the run loop tests it once after each block (`ARM.cpp:639-655`). An I/O write that raises an IRQ is seen at the block end, up to 32 instructions late.

### Invalidation of written code

Code memory is tracked in 512-byte ranges with a 32-bit mask of 16-byte pieces (`ARMJIT.cpp:616-635`, `911-925`). Every write to RAM that can hold code checks that mask (`ARMJIT.h:58-64`; for example `NDS.cpp:2117`), and a hit removes the blocks that cover those 16 bytes from the lookup table (`ARMJIT.cpp:937-1029`). With fastmem, write protection on code pages does the same job for compiled stores. ITCM and ARM7 VRAM changes are checked in bulk (`ARMJIT.cpp:1031-1063`).

### Thumb

Thumb uses the same scanner, register cache and cycle model with its own compile table (`ARMJIT_A64/ARMJIT_Compiler.cpp:614-650`). The two halves of a Thumb BL are merged into one pseudo-instruction (`ARMJIT.cpp:731-741`). PC-relative Thumb loads take the literal path above (`ARMJIT.cpp:282-286`).

## Dynarmic

Dynarmic is the ARM-to-host JIT behind Citra-family 3DS emulators and Yuzu-family Switch emulators. It decodes ARM into its own typed SSA IR, runs a few passes, and emits x64 (with Xbyak) or A64 (with oaknut) (`docs/Design.md`).

- **Block.** A block is a basic block of IR. Translation stops when an instruction ends it, when the condition state cannot continue, or when single-stepping (`src/dynarmic/frontend/A32/translate/translate_arm.cpp:28-72`). The terminal says where control goes: `LinkBlock`, `LinkBlockFast`, `PopRSBHint`, `FastDispatchHint`, `ReturnToDispatch`, `Interpret`, or an `If` on the flags (`docs/Design.md:287-324`).
- **Cycles.** Each instruction adds its ticks to the block's `CycleCount` at translation time (`translate_arm.cpp:31-64`). The emitter subtracts that sum from `cycles_remaining` once per block (`src/dynarmic/backend/x64/a32_emit_x64.cpp:159-161`, `src/dynarmic/backend/x64/emit_x64.cpp:274-277`). A `LinkBlock` terminal then compares `cycles_remaining` with zero and jumps to the next block only while time is left (`a32_emit_x64.cpp:1157-1189`). There is no check inside a block.
- **Registers.** Guest state lives in a `JitState` in memory, reached through a pinned register (r15 on x64); r14 holds the page table and r13 the fastmem base (`src/dynarmic/backend/x64/block_of_code.cpp:320`, `src/dynarmic/backend/x64/emit_x64_memory.h:86`, `159`). Within a block, `GetRegister`/`SetRegister` microinstructions are cut down by the get/set elimination pass, then constant propagation and dead-code elimination (`src/dynarmic/backend/x64/a32_interface.cpp:217-228`). The host register allocator works on IR values with use counts (`docs/RegisterAllocator.md`). Like melonDS, nothing stays in host registers between blocks.
- **Linking and dispatch.** A link is a patchable `jg` (or `jmp` for `LinkBlockFast`) straight to the target block's code; when the target is compiled later, the jump is patched, and invalidation unpatches it (`a32_emit_x64.cpp:1248-1297`). Function returns use an 8-entry return stack buffer (`src/dynarmic/backend/x64/a32_jitstate.h:48-52`), and indirect jumps use a 65536-entry MRU table searched in assembly (`src/dynarmic/backend/x64/a32_emit_x64.h:69-71`, `a32_emit_x64.cpp:214-275`). The slow dispatcher checks `halt_reason` and the remaining cycles, then calls `LookupBlock` (`block_of_code.cpp:355-381`).
- **Memory.** Three modes: callbacks for every access, an inline page-table walk, or fastmem (`src/dynarmic/backend/x64/emit_x64_memory.cpp.inc:54-140`). The page-table walk is a shift, an indexed load, a null test and an add (`emit_x64_memory.h:75-99`). The slow path is deferred: it is emitted after the block's main code, so the fast path is a straight line (`emit_x64_memory.cpp.inc:96-121`). On a fastmem fault the handler sends execution to the fallback, and can mark that access "do not fastmem" and invalidate the block so the next compile uses the slow path there (`emit_x64_memory.cpp.inc:17-52`).
- **Halting.** The host stops the JIT by setting `halt_reason`, which the dispatcher and the `CheckHalt` terminal test (`a32_emit_x64.cpp:1242-1246`, `block_of_code.cpp:355-366`). Interrupts are the host's job between runs.

## DeSmuME

DeSmuME's JIT (`desmume/src/arm_jit.cpp`) is the closest relative of ours: a DS JIT that uses a library code generator with its own register allocator (AsmJit's compiler) instead of a hand emitter.

- A block ends at any branch or at the configured maximum size (`arm_jit.cpp:4142`).
- Each instruction adds its fixed cycles to a compile-time sum, `bb_constant_cycles`; only instructions with variable timing add at run time; the block adds the sum once and returns the total (`arm_jit.cpp:4122-4204`). There is no budget check inside a block.
- Guest registers are read and written as memory operands on the CPU struct, `reg_ptr` and `cpu_ptr` (`arm_jit.cpp:275-282`). Instructions without a compiler call the interpreter's function for that opcode (`emit_armop_call`).
- Lookup is a flat array of function pointers indexed by address (`desmume/src/arm_jit.h:66-76`).
- Self-modifying code that recompiles too often falls back to the interpreter for that address (`arm_jit.cpp:4241-4252`).

## Dolphin

Dolphin's JITs (Jit64 for x64, JitArm64) translate GameCube and Wii PowerPC code. Paths are relative to `Source/Core/Core/` in dolphin-emu/dolphin 4a70679 (2026-11-06), read from a sparse clone.

- **Block.** A block ends at an instruction flagged `FL_ENDBLOCK` (`PowerPC/PPCAnalyst.cpp:243`). The analyser follows up to two branches per block (`BRANCH_FOLLOWING_THRESHOLD`, `PowerPC/PPCAnalyst.cpp:42`, logic at `888-975`), and a `blr` that returns from a followed `bl` is followed too. Idle loops are never followed (`PPCAnalyst.cpp:970-974`).
- **Cycles.** `js.downcountAmount` starts at zero for each block (`PowerPC/Jit64/Jit.cpp:979`), and each instruction adds its `num_cycles` at compile time (`Jit.cpp:1031`). Every exit subtracts the sum from the downcount with one `SUB` (`Jit.cpp:607`, `667`, `713`, `735`, `748`), then goes to the timing code if the downcount reached zero, or else to the next block (`Jit.cpp:594-650`). The dispatcher reuses the flags of that `SUB` (`PowerPC/Jit64/JitAsm.cpp:87-89`). A slice runs up to 20000 cycles (`CoreTiming.cpp:37`, `378`), and events fire only between blocks.
- **Linking.** The jump to the next block starts as a jump to `dispatcher_no_timing_check` and is patched to the target block once that exists (`Jit.cpp:594-650`).
- **Registers.** Two host registers are pinned: RBP points into the PowerPC state and RBX holds the memory base (`PowerPC/Jit64Common/Jit64Constants.h:17-21`). The register cache picks a spill victim by looking up to 64 instructions ahead, with a penalty for dirty registers (`PowerPC/Jit64/RegCache/JitRegCache.cpp:536-560`), and flushes at exits (`JitRegCache.cpp:333`).
- **Flags.** A backward pass marks which carry and condition-register bits a later instruction reads, and the emitter skips the rest (`PPCAnalyst.cpp:1015-1053`).
- **Lookup.** The assembly dispatcher indexes an entry-point table by feature flags and PC (`JitAsm.cpp:110-125`). The table is a lazily committed reservation of `0x10'0000'0000` bytes, with a 65536-entry fallback where the reservation fails (`PowerPC/JitCommon/JitCache.h:121-123`, `200-210`).
- **Fastmem.** The arena holds a 4 GiB physical view and a 4 GiB logical view between 2 GiB guard regions, and the memory base register points at one view or the other by the MSR's DR bit. An access to an unmapped page faults, and the handler patches that access into a slow call (`HW/Memmap.cpp:195-233`). On ARM64 the patch writes a `BL` to the slow-access code and flushes the instruction cache (`PowerPC/JitArm64/JitArm64_BackPatch.cpp:333-365`).
- **Idle loops.** `IsBusyWaitLoop` (`PPCAnalyst.cpp:758-800`) accepts a loop back to its own block start with no other branch, no CTR use, only integer and load instructions, and no register written after an earlier read in the loop. melonDS copied this rule (its `ARMJIT.cpp:355` says so). When such a loop runs, `Idle()` adds the remaining downcount to the idle cycles and zeroes it (`CoreTiming.cpp:574-588`).

The following come from search summaries, because the blog pages refused the web reader and these files are outside the sparse clone; they were not checked line by line. Loads and stores with a constant address are folded, constant-address MMIO gets inline handlers through `MMIOReadCodeGenerator`, and the GPU FIFO has a fast path (`PowerPC/Jit64Common/EmuCodeBlock.cpp`). Code invalidation follows the guest's own `icbi` and `dcb*` instructions, with no detection of writes (`PowerPC/MMU.cpp`). A fastmem access is as few as 2 instructions against up to 1000 for the slow path (https://dolphin-emu.org/blog/2016/09/06/booting-the-final-gc-game/). The 2407-2409 progress report says the Cached Interpreter 2.0 is 3 to 4 times faster than the interpreter but an order of magnitude slower than the JITs, because "an interpreter cannot keep guest registers in host registers" (https://dolphin-emu.org/blog/2024/09/04/dolphin-progress-report-release-2407-2409/).

## Cranelift as a JIT backend

Cranelift is a code generator built first for WebAssembly runtimes. Local paths below are relative to the crates.io sources of `cranelift-codegen-0.137.1` and `cranelift-codegen-meta-0.137.1`.

- **Settings.** `regalloc_algorithm` is `backtracking` (the default, and ours) or `single_pass`, which its own doc describes as quick compilation with more register spills and moves (`cranelift-codegen-meta-0.137.1/src/shared/settings.rs:33-45`). `opt_level` is `none`, `speed` or `speed_and_size` (`settings.rs:47-58`), and `none` skips the e-graph optimiser (https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/cranelift/codegen/src/context.rs). `enable_pinned_reg` reserves one host register that code reads and writes with `get_pinned_reg` and `set_pinned_reg` (`settings.rs:112-121`): x21 on aarch64 (`isa/aarch64/inst/regs.rs:20`) and r15 on x64 (`isa/x64/abi.rs:1339`). That is one register, where melonDS pins four.
- **Calling conventions.** The conventions are a fixed enum: Fast, Tail, SystemV, WindowsFastcall, AppleAarch64, Probestack, Winch and PreserveAll (`isa/call_conv.rs:11-62`). A JIT cannot define its own, such as one where guest registers arrive in chosen host registers. Only `Tail` supports tail calls (`call_conv.rs:91-97`). Its arguments on aarch64 start at x2 (`isa/aarch64/abi.rs:178`), and on x64 `return_call` needs `preserve_frame_pointers` (`isa/x64/inst/emit.rs:1887-1888`). `PreserveAll` clobbers nothing at the call site but cannot return values or tail call (`call_conv.rs:50-62`). Tail calls landed in 2023 (RFC at https://github.com/bytecodealliance/rfcs/blob/main/accepted/tail-calls.md) and are on for x86_64, aarch64 and riscv64 since wasmtime PR 8540 (https://github.com/bytecodealliance/wasmtime/pull/8540). So blocks can be chained: each block a `Tail` function that ends in a `return_call` to the next.
- **Compile cost.** Chris Fallin, who wrote its register allocator, says most compile time goes to the register allocator, and that its code is maybe 10 to 30% slower than LLVM's (https://news.ycombinator.com/item?id=39742692). A CGO 2024 paper on database query compilation found Cranelift only 20 to 35% faster to compile than compile-time-tuned LLVM, and a single-pass compiler 16 times faster than Cranelift (https://dl.acm.org/doi/10.1109/CGO57630.2024.10444856). The TPDE paper measures TPDE compiling 4.27 times faster than default Cranelift and 2.68 times faster than Cranelift with the single-pass allocator (https://arxiv.org/html/2505.22610). A Google Summer of Code report on the single-pass allocator gives 1.07 to 5 times faster compiles and 1.06 to 7.5 times slower code on Sightglass (https://d-sonuga.netlify.app/gsoc/regalloc-iii/). The docs advise reusing one `Context` with `clear()` (https://docs.rs/cranelift-codegen/latest/cranelift_codegen/struct.Context.html), which we do (`gba/cpu/jit/emit.rs:94`). Our blocks cost 0.70 ms (MK) and 0.43 ms (HG) each on the Mac.
- **Faults.** Cranelift records trap sites (offsets in the compiled buffer's `traps()`, with trap codes from `MemFlags`) but installs no handler. Wasmtime maps the PC of a SIGSEGV to its trap table itself (https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/crates/wasmtime/src/runtime/vm/traphandlers.rs). Fastmem on Cranelift would need our own signal handler and a resume point for every access. No published write-up of fastmem on Cranelift turned up.

Other guest JITs on Cranelift:

- **pimu** (AArch64 guest) compiles runs of guest instructions, 5.3 on average, at about 0.5 ms each on x86 and about 6 ms on a Raspberry Pi 4, and chains them through `CallConv::Tail` with a SystemV entry trampoline. It never beat its interpreter (an Ubuntu boot took 604 s against 530 s), and the issue was closed as not planned (https://github.com/rusty-pi/pimu/issues/209). It is the case closest to ours.
- **riscv-emu** compiles hot traces of up to 128 instructions after 50 visits, in the background. Registers are SSA values written through to memory, exits tail-call the next trace through a jump cache, and the TLB lookup is inline. A Linux boot takes 1.76 s against 7.97 s interpreted, and its stated problem is the register allocator's compile time (https://github.com/evanphx/riscv-emu).
- **Microlift** compiles regions as functions that take a register-context pointer, with link cells, pinned registers, a call-return stack and a page-indexed cache. It reports about 20 times from linking, and needed a Cranelift fork with a wider register ABI to pin registers across linked blocks (https://github.com/microlift/microlift).
- Icicle (https://arxiv.org/html/2301.13346v1) and rvtime (https://docs.rs/rvtime-cranelift/latest/rvtime_cranelift/) also emit through Cranelift.

For us, the run-time gap comes from what we hand Cranelift. The four reference JITs move bookkeeping to compile time and out of the block, so each guest instruction becomes fewer host operations. Cranelift's own limits matter in two places: compile time per block, and the fixed conventions, which allow one pinned register and no custom argument registers.

## Our JIT

What the code does today, with the places that cost.

- **Blocks.** At most 128 instructions inside one 1 KiB page (`gba/cpu/jit.rs:27-28`, `479`). A block ends at every B, BL, BX and every other PC write (`gba/cpu/jit/scan.rs:38-43`). `MOV PC, LR`, loads into the PC other than LDM, MRC/MCR and SWI run in the interpreter and end the block (`Op::Exit`). A block compiles after its third visit (`HOT`, `gba/cpu/jit.rs:26`) on a worker thread while the interpreter runs (`gba/cpu/jit.rs:337-366`). Any instruction of a block can be its entry: the function starts with a `br_table` on the start index (`gba/cpu/jit/emit.rs:439-458`), and every inner PC gets its own table entry (`gba/cpu/jit.rs:419-431`).
- **Per-instruction exit check.** After every instruction the code compares the running cycle total with the budget and tests a force flag, and branches to the exit if either is set (`gba/cpu/jit/emit.rs:552-581`). The exit block takes the PC, both pipeline words, the retired count and the next-fetch kind as values from every one of those branches.
- **Cycle model.** Each instruction runs the ARM9 code/data overlap model at run time: a `umin`, two subtracts and adds for the debt, then the total (`gba/cpu/jit/emit/timing.rs:63-115`). This keeps the clock equal to the interpreter's after every instruction.
- **Registers.** Guest registers are Cranelift variables. The entry loads every register the block uses, and each exit stores every register the block writes, plus the CPSR (`gba/cpu/jit/emit.rs:395-411`). An instruction run through the interpreter helper (`Op::Step`) stores all of them, calls, and reloads all of them (`gba/cpu/jit/emit.rs:599-618`).
- **Flags.** Every flag-setting instruction rebuilds N, Z, C and V into the CPSR word (`gba/cpu/jit/emit/alu.rs:184-212`), whether or not anything reads them. Nearly every Thumb ALU instruction sets flags.
- **Memory.** ARM9 loads and stores try three windows in turn (ITCM, DTCM, main RAM), each with two table loads, a subtract, a compare and a branch, then the access; main RAM adds a data-cache flag lookup for timing; stores add a code-page check and a dirty-bit update (`gba/cpu/jit/emit/memory.rs:35-237`). The ARM7 has no window at all, so every ARM7 load and store calls a Rust helper (`gba/cpu/jit/emit/memory.rs:139-141`). Any access outside RAM (I/O, the GBA slot) makes the helper set the exit flag (`gba/cpu/jit/helpers.rs:73-75`, `77-109`), so the block ends there. LDM and STM go register by register (`gba/cpu/jit/emit/block.rs`).
- **Branches.** A taken branch calls the `branch` helper, which runs `Core::reload`: two code fetches through the bus and its instruction-cache model (`gba/cpu/jit/emit/branch.rs:6-62`, `gba/cpu/jit/helpers.rs:111-125`, `gba/cpu.rs:322-336`). Then the block returns.
- **Dispatch.** `Jit::run` loops over blocks inside one call (`gba/cpu/jit.rs:190-238`). Each entry does a `HashMap` lookup (`gba/cpu/jit.rs:272-322`), checks the generation of every code page the block covers (`current`, `gba/cpu/jit.rs:555-581`), compares the two pipeline words with the block's (`enters`, `gba/cpu/jit.rs:96-100`), clears the code-hit flag, and calls the block with five arguments. Around that, the DS loop runs `run_jit` per attempt with its own debugger, IRQ and budget checks (`nds/machine.rs:658-710`), and slices are 64 system cycles (`nds/machine.rs:76`, `485-512`).
- **Idle loops.** Only a branch to itself is skipped (`nds/machine.rs:616-656`).
- **Code writes.** Writes mark 1 KiB pages; a block is re-validated on entry by page generation, and by a word compare when a generation moved (`gba/cpu/jit.rs:555-581`).
- **Compiler.** Cranelift with `opt_level=speed`, the default backtracking register allocator, verifier off in release (`gba/cpu/jit/emit.rs:71-90`). The JIT Compile Cost brief recorded 0.70 ms (MK) and 0.43 ms (HG) per block on a Mac after the inline memory paths landed.

## Ranked comparison

Ranked by expected cost to our frame time. The host-operation counts in row 1 are read off the emitter source; no disassembly has been taken.

| Rank | Design choice | melonDS | Ours | Expected cost | Survives exact timing |
| --- | --- | --- | --- | --- | --- |
| 1 | Cycle bookkeeping and exit checks inside a block | One compile-time sum, one `ADD` at the exit, no check inside; a block may overshoot the slice (`ARMJIT_A64/ARMJIT_Compiler.cpp:820-824`, `ARM.cpp:617`) | Overlap model per instruction at run time, then a budget compare, a force-flag test and a branch after every instruction; each branch passes the exit block its PC, pipeline words, retired count and fetch kind (`gba/cpu/jit/emit.rs:552-581`, `gba/cpu/jit/emit/timing.rs:63-115`) | High: about 8 to 10 host operations of bookkeeping per guest instruction, where a whole melonDS ALU instruction is 1 to 3 | Yes, with one check per run of instructions (change 1) |
| 2 | Block length and what ends a block | Up to 32 instructions; follows static B, BL and BX LR and the taken side of conditional branches; I/O does not end a block (`ARMJIT.cpp:743-814`) | Up to 128, but every taken branch and every I/O access ends the block (`gba/cpu/jit/scan.rs:38-43`, `gba/cpu/jit/helpers.rs:73-109`) | High: the dispatcher is 10% of an HG frame | Yes, with the refetch timing inline and an exit after I/O only when IRQ, halt or DMA state changed (change 3) |
| 3 | Cost of entering a block | Flat table, one load and compare, no validation (`ARM.cpp:617-656`, `ARMJIT.cpp:932-934`) | `HashMap` lookup, generation check of every covered page, pipeline compare, flag clears, a five-argument call, loads of every used register (`gba/cpu/jit.rs:272-322`, `555-581`, `96-100`; `gba/cpu/jit/emit.rs:395-411`) | High: paid on every entry, and our blocks are short | Yes (change 4) |
| 4 | Entry points | One per block, as in Dynarmic, DeSmuME and Dolphin | Any instruction, through a `br_table` (`gba/cpu/jit/emit.rs:439-458`, `gba/cpu/jit.rs:419-431`) | Medium to high: every instruction is a merge point with block parameters for each live variable, which adds moves and compile time | Yes (change 2) |
| 5 | Branch timing | Compile time for static targets; a short stub for register targets (`ARMJIT_A64/ARMJIT_Branch.cpp:36-166`, `169-228`) | A Rust helper runs `Core::reload`, two code fetches through the bus and the instruction-cache model (`gba/cpu/jit/emit/branch.rs:6-62`, `gba/cpu.rs:322-336`) | Medium | Yes, with the instruction-cache lookup emitted inline like the data-cache one |
| 6 | Flag updates | Only flags a later instruction reads (`ARMJIT.cpp:247-263`); EQ to VC is one `TBZ` or `TBNZ` | N, Z, C and V rebuilt for every flag-setting instruction (`gba/cpu/jit/emit/alu.rs:184-212`) | Medium: nearly every Thumb ALU instruction sets flags | Yes (change 5) |
| 7 | PC-relative literal loads | A constant at compile time, guarded by a literal hash (`ARMJIT_A64/ARMJIT_LoadStore.cpp:64-105`, `ARMJIT.cpp:703-728`) | A normal load through the three windows | Medium to low | The value yes; the timing still needs the data-cache lookup |
| 8 | Memory accesses | On the Mac a call per access, one call per LDM or STM (`ARMJIT_A64/ARMJIT_LoadStore.cpp:259-330`, `ARMJIT.cpp:181-192`) | ARM9: three inline windows in turn, about 20 operations to reach main RAM, plus cache timing and store checks; ARM7: a helper call per access; LDM and STM per register (`gba/cpu/jit/emit/memory.rs:35-237`) | Medium: ARM9 is close to melonDS on the Mac; the ARM7 is about 4% of the frame | Yes |
| 9 | Idle loops | Polling loops by Dolphin's rule, JIT only (`ARMJIT.cpp:353-382`, `ARM.cpp:639-655`) | A branch to itself only (`nds/machine.rs:616-656`) | Depends on the game | Only exact skips; backlog item 83di covers it for JIT and interpreter both |
| 10 | Compiling | Synchronous, one pass, no IR, on the first visit (`ARM.cpp:632-637`) | Cranelift on a worker thread after the third visit, 0.43 to 0.70 ms per block (`gba/cpu/jit.rs:26`, `337-366`) | Low once warm; high on new code, where frames pass 16.7 ms | Yes |
| 11 | Code-write tracking | 16-byte pieces, checked on the write, nothing on entry (`ARMJIT.cpp:911-1029`) | 1 KiB pages, a generation check on every entry and a word compare after a data write into a code page (`gba/cpu/jit.rs:555-581`) | Low to medium, counted in row 3 | Yes |
| 12 | Guest registers across blocks | Written back at every exit | Written back at every exit; Cranelift variables within a block | None, not a gap | Not affected |

## Five changes

Each change keeps the clock and core state equal to the interpreter's at every block end, so lockstep still passes.

### 1. One budget check per run of instructions

Split a block into runs that end at an instruction whose cost is known only at run time: a memory access or a helper call. Fold each run's fetch costs and the ARM9 code and data overlap at compile time wherever the inputs are known. At the head of a run, one compare: if the clock plus the cost of every instruction in the run but the last stays under the budget, no per-instruction check inside the run would fire, so the run executes with none. Otherwise a cold path runs today's per-instruction checks or exits to the interpreter. Test the force flag after helper calls, which is where the helpers set it (`gba/cpu/jit/helpers.rs:73-75`).

A slice is 64 system cycles, 128 ARM9 cycles (`nds/machine.rs:76`), so the cold path runs about once per slice.

Evidence: melonDS (`ARMJIT_A64/ARMJIT_Compiler.cpp:820-824`), Dynarmic (`a32_emit_x64.cpp:159-161`), DeSmuME (`arm_jit.cpp:4122-4204`) and Dolphin (`Jit.cpp:1031`, `607`) add cycles once per block and check nothing inside it.

Cost today: about 8 to 10 host operations per guest instruction (`gba/cpu/jit/emit.rs:552-581`, `gba/cpu/jit/emit/timing.rs:63-115`), and every one of those branches passes values to the exit block, which the register allocator has to merge.

### 2. Single-entry blocks

Drop the `br_table` and the inner-PC entries. A jump into the middle of a block compiles a new block that starts there, as in melonDS (`ARMJIT.cpp:544-566`, `927-930`).

Evidence: melonDS, Dynarmic, DeSmuME and Dolphin all enter a block only at its start.

Cost today: every instruction is a merge point with block parameters for each live variable (`gba/cpu/jit/emit.rs:439-458`, `gba/cpu/jit.rs:419-431`). That adds moves at run time and register-allocator work at compile time, where most of Cranelift's compile time goes. Change 1 depends on it, because a run with an entry in its middle cannot check its budget once at the head.

### 3. Longer blocks

Follow static B and BL, and BX LR when LR was set earlier in the block, as melonDS does (`ARMJIT.cpp:743-814`, `322-348`); Dolphin follows up to two branches (`PPCAnalyst.cpp:42`, `888-975`). Emit the branch refetch timing inline, with the instruction-cache lookup done the way the data-cache lookup is done today. A backward branch to the block's own start becomes a Cranelift loop with the run check at its head, the same shape as a Dynarmic `LinkBlock` terminal that targets its own block (`a32_emit_x64.cpp:1157-1189`). An I/O access stays inside the block unless the helper reports that IRQ, halt or DMA state changed, which is when the interpreter's next step would differ.

Cost today: every taken branch ends the block and returns to the dispatcher (`gba/cpu/jit/scan.rs:38-43`, `gba/cpu/jit/emit/branch.rs:6-62`), and so does every I/O access (`gba/cpu/jit/helpers.rs:73-109`). The dispatcher is 10% of an HG frame.

### 4. Flat lookup table, invalidation on write, then chaining

Replace the `HashMap` lookup and the checks on entry with a flat table indexed by PC, as in melonDS (`ARMJIT.cpp:932-934`), Dolphin (`PowerPC/JitCommon/JitCache.h:121-123`) and DeSmuME (`arm_jit.h:66-76`). Remove a block from the table when its code is written, so a table hit needs no check; keep the pipeline-word compare only if self-modifying code still needs it. Then chain blocks: each block a function in Cranelift's `Tail` convention that ends in a `return_call` to its successor (x64 also needs `preserve_frame_pointers`), or a jump cache as in riscv-emu. pimu chains through `Tail`, and Microlift reports about 20 times from linking. Lockstep compares at every block end, so in lockstep builds a chained exit has to return to the dispatcher, or the comparison has to move into the chained exit.

Cost today: on every entry a `HashMap` lookup, a generation check of every covered page, a compare of both pipeline words, flag clears and a five-argument call (`gba/cpu/jit.rs:272-322`, `555-581`, `96-100`), once per short block.

### 5. Flag liveness

Compute N, Z, C and V only when a later instruction reads them before they are set again, before a helper call that reads the CPSR, and at the block exit. melonDS does this with `FloodFillSetFlags` (`ARMJIT.cpp:247-263`), Dynarmic with get/set elimination (`src/dynarmic/backend/x64/a32_interface.cpp:217-228`) and Dolphin with a backward pass over carry and condition-register bits (`PPCAnalyst.cpp:1015-1053`).

Cost today: every flag-setting instruction rebuilds all four flags into the CPSR word (`gba/cpu/jit/emit/alu.rs:184-212`), and nearly every Thumb ALU instruction sets flags.

Close behind as a sixth: load PC-relative literals as compile-time constants, guarded by a literal hash and an invalid-literal list as in melonDS (`ARMJIT_A64/ARMJIT_LoadStore.cpp:64-105`, `ARMJIT.cpp:703-728`). The value becomes exact at compile time; the access timing still needs the data-cache lookup.

### Not in the five

- Fastmem. melonDS has none on the Mac, where our gap is measured, and Cranelift has no fault handler to build it on.
- Replacing Cranelift. The run-time gap comes from what we emit. Compile cost is still a risk: pimu's runs of 5.3 instructions never paid back their compile time. Changes 2 and 3 make blocks longer, which helps that ratio.
- Idle loops. Backlog item 83di covers them, and they help the interpreter too.
- ARM7 memory windows. The ARM7 is about 4% of the frame.

### Exact timing

Relaxing exactness as melonDS does (first-run cycle costs, overshooting the slice, IRQs only at block ends) would make changes 1 to 3 simpler, but it breaks lockstep. The recommendation is to keep timing exact and do changes 1 to 5 first.

Ticket 83ic took the overshoot part in a form lockstep accepts: the slice ends at guest-defined end points (see the constraint above), the same in every engine. Cycle costs and IRQ timing stay exact. It removed the short-budget block copies and the near entries on DS.

### Measurements that would confirm this

Neither has been run.

- A Cranelift disassembly of one hot block (`set_disasm`, the vcode in `CompiledCode`), to count host operations per guest instruction and the moves at the `br_table` merge points.
- A melonDS run with `IsIdleLoop` returning false, to split its JIT ratio into faster code and idle skipping.
