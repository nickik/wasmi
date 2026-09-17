# Cosmic SIA32 Port TODO

## Goal

Run a capability-confined WebAssembly module in the freestanding SIA32 Cosmic kernel using
`wasmi` as a Rust `no_std + alloc` interpreter.

The first production acceptance target is intentionally interpreter-only:

1. Cosmic boots through the real SIA32 reset/ROM/MMU/PLIO path.
2. The kernel loads a static `.wasm` module from the Cosmic image or object store.
3. It instantiates the module with only the approved `cosmic:sys` imports.
4. An exported `_start` calls `log_write` and returns successfully.
5. Fuel exhaustion and a trapped module terminate only that module, with a clear kernel diagnostic.

SIA native compilation is a later, separately gated AOT path. It must not delay the interpreter
or require executable-memory/JIT support in Cosmic.

## Verified baseline and external gates (2026-09-17)

- **PROVEN:** `wasmi` builds with the intended interpreter features on Rust nightly 1.100.0:
  `cargo check -p wasmi --no-default-features --features validate,deterministic,portable-dispatch,indirect-dispatch,extra-checks`.
  Its normal dependency graph contains no `std` feature.
- **BLOCKED before crate compilation:** the available Rust compiler has no
  `sia32-unknown-cosmic` target specification. The exact SIA command fails while rustc queries
  target metadata, so a Wasmi-side CI target cannot yet be made green.
- **BLOCKED after target registration:** Wasmi intentionally uses Rust `u64`/`i64` pervasively
  for values, fuel, and table/memory accounting. The current Cranelift SIA32 backend rejects I64
  lowering. Disabling Wasm `memory64` does not remove this Rust requirement.
- **Cosmic placement gate:** Cosmic's authoritative `AGENTS.md` requires production kernel/OS
  policy in Forge and forbids a permanent Rust kernel. The Wasmi runtime must therefore be
  defined as an unprivileged service/module with a narrow Cosmic ABI, or Cosmic must explicitly
  approve a different placement. Do not add a `cosmic-wasmi` Rust crate to the privileged kernel.
- **Native-path gate:** Cosmic has not yet reached M27--M29 (freestanding Forge profile,
  SIA image/link ABI, and real native boot). LightingSimulation's existing Pico boot harness
  accepts assembly-built CSM1 images; it cannot yet ingest a compiler-produced Rust/SIA image.

The next executable prerequisite is Rust-on-SIA support for a freestanding target with
`alloc` and correct generated I64 support. The runtime work resumes immediately after that gate
is green. Meanwhile W0 may define the capability ABI and conformance vectors without freezing a
kernel embedding mechanism.

## Non-goals for the first milestone

- WASI, POSIX emulation, filesystem or network imports.
- JIT compilation in the kernel.
- Threads, shared memory, GC, exceptions, component model, or dynamic linking.
- Floating-point/SIMD support until the SIA execution and compiler paths support them end-to-end.
- Giving modules raw kernel pointers, raw physical addresses, or unrestricted syscalls.

## Phase 0 — Freeze the target and ABI

The proposed unprivileged runtime boundary and initial `cosmic:sys` contract live in
[`docs/COSMIC_SIA_RUNTIME_CONTRACT.md`](docs/COSMIC_SIA_RUNTIME_CONTRACT.md). It is not frozen
until Cosmic adopts an equivalent contract.

### Completed host-side W0 evidence

- [x] Checked-in binary fixtures plus reproducible `.wat` sources for `log_write` success,
  bounds rejection, denied capability, and fuel exhaustion. The conformance test proves each
  source regenerates its exact binary fixture.
- [x] Host manifest/reference-loader checks for module hash, ABI version, byte/import/memory
  limits, malformed payloads, unknown/incompatible imports, and repeated lifecycle cleanup.
- [x] Focused reference-harness command:
  `cargo test -p wasmi cosmic_runtime --no-default-features --features wat,validate,deterministic,portable-dispatch,indirect-dispatch,extra-checks`.

- [x] Published remote PR: [PR #1](https://github.com/nickik/wasmi/pull/1), latest verified runtime commit `cad70cbbc95314d752b9e03a9288f8d3b1c8a35a`.
- [x] Exact-head host test: `cargo test -p wasmi cosmic_runtime --no-default-features --features wat,validate,deterministic,portable-dispatch,indirect-dispatch,extra-checks` — 13 passed on `cad70cbbc95314d752b9e03a9288f8d3b1c8a35a`.
- [ ] CI evidence: `.github/workflows/rust.yml` defines a `pull_request` gate for `main`, but GitHub reports no status checks and zero pull-request workflow runs for `cad70cbbc95314d752b9e03a9288f8d3b1c8a35a`.

- [ ] Define the initial target triple/specification for freestanding SIA32 Rust:
  pointer width, endianness, stack alignment, atomics, panic strategy, and supported Rust version.
- [ ] Prove a minimal `#![no_std]`, `#![no_main]` Rust binary executes on the real LightingSimulation
  SIA32 path and reports a deterministic result.
- [ ] Provide Cosmic's global allocator and allocator-error policy before linking `wasmi`.
- [ ] Define the initial `cosmic:sys` ABI as versioned capability imports, not syscall numbers:
  `log_write(handle, ptr, len)`, `yield()`, and explicit future capability families.
- [ ] Specify pointer/length validation, UTF-8 policy for logging, maximum import count,
  maximum linear-memory pages, stack/depth limits, and per-module fuel budget.
- [ ] Define module packaging: `module.wasm`, manifest, requested capabilities,
  content hash, and an initial static image-loader format.
- [ ] Write the threat model: malformed Wasm, deliberate fuel/memory exhaustion,
  invalid host pointers, host-import failure, and module traps.
- [ ] Resolve the runtime placement decision with Cosmic: unprivileged Wasmi service/module
  ABI versus an explicitly approved alternative. The privileged Forge kernel remains the
  capability and resource-policy authority in every case.

**Exit:** one reviewed ABI document shared by Cosmic and this fork.

## Phase 1 — Make the Wasmi core compile for SIA32

The intended first dependency configuration is:

```toml
wasmi = {
  path = "../wasmi/crates/wasmi",
  default-features = false,
  features = [
    "validate",
    "deterministic",
    "portable-dispatch",
    "indirect-dispatch",
    "extra-checks",
  ],
}
```

Start without `std`, `wat`, `memory64`, `simd`, or WASI. Re-evaluate
`extra-checks` after the safety/performance measurements; it is enabled initially for defense in depth.

- [ ] Add a `sia32-unknown-cosmic` CI target/configuration owned by this fork.
- [ ] Produce a dependency-feature report proving no selected crate enables `std`.
- [ ] Cross-compile `wasmi_core`, `wasmi_ir`, `wasmi_collections`, and `wasmi`
  with the exact production feature set.
- [ ] Resolve missing target intrinsics, compiler-builtins, atomics, unwind, formatting,
  allocation, or linker dependencies without adding `std`.
- [ ] Confirm the selected dispatch is portable on SIA32; never rely on LLVM tail-call dispatch
  without a target-specific proof.
- [ ] Confirm the interpreter does not assume unaligned host loads/stores that the SIA path cannot support.
- [ ] Add a build-size report for the minimal runtime and a static memory-budget report.
- [ ] Keep upstream-compatible changes isolated; document every SIA/Cosmic conditional or patch.

**Exit:** `cargo build --target sia32-unknown-cosmic --no-default-features` succeeds for the
minimal interpreter dependency crate, and CI records the exact feature graph.

## Phase 2 — Create the Cosmic runtime adapter

- [ ] Add a small `cosmic-wasmi` adapter at the selected **unprivileged** runtime/module
  boundary, depending on this fork by pinned revision. It must not become privileged Cosmic
  kernel policy code.
- [ ] Implement a no-heap-host-state path except for the allocator explicitly supplied by Cosmic.
- [ ] Construct `Engine`, `Store`, `Linker`, `Module`, `Instance`, and typed entry-point
  invocation without `std` convenience APIs.
- [ ] Enable Wasmi fuel metering and map exhaustion to a stable `CosmicWasmError::FuelExhausted`.
- [ ] Map Wasm traps and validation/instantiation failures to stable error codes and kernel diagnostics.
- [ ] Implement linear-memory range validation before every host read/write:
  no integer overflow, no out-of-bounds access, no aliasing assumptions.
- [ ] Implement `cosmic:sys/log_write` behind a logging capability; reject unknown imports,
  imports with incompatible signatures, and missing required capabilities.
- [ ] Add a module lifecycle: load → validate → instantiate → call → trap/return → destroy.
- [ ] Ensure module cleanup frees Wasm allocations and drops capability state after success and every failure path.

**Exit:** a minimal hand-authored integer-only `.wasm` module executes under a Cosmic unit/integration harness.

## Phase 3 — Real SIA32 boot-path integration

- [ ] Teach the Cosmic image builder to embed one named Wasm module and manifest.
- [ ] Load it after the System Task and allocator are alive, through the normal image/object path.
- [ ] Execute `_start` from the real freestanding kernel, not a host-side mock.
- [ ] Emit a deterministic PLIO/serial diagnostic containing module ID, ABI version,
  success/trap result, consumed fuel, and final memory-page count.
- [ ] Add LightingSimulation acceptance tests for:
  - [ ] successful `log_write` plus return;
  - [ ] integer arithmetic and structured control flow;
  - [ ] memory read/write at valid bounds;
  - [ ] out-of-bounds memory trap;
  - [ ] fuel exhaustion;
  - [ ] denied import/capability;
  - [ ] malformed module rejected before instantiation;
  - [ ] repeated load/run/destroy with no allocator leak.
- [ ] Run these through the real reset/ROM/MMU/PLIO route and retain machine-readable traces.

**Exit:** the Phase 0 acceptance target passes in CI on the real simulator route.

## Phase 4 — Harden the interpreter deployment

- [ ] Add deterministic resource accounting: module bytes, translated bytecode, instances,
  tables, globals, call depth, linear memory, host-call bytes, and fuel.
- [ ] Define fair scheduling/yield behavior so one module cannot monopolize the System Task.
- [ ] Add module teardown for task exit, fault, capability revocation, and image unload.
- [ ] Add a corpus of invalid and adversarial modules plus Wasm spec regression subsets that fit Cosmic.
- [ ] Differential-test selected modules on host Wasmi and the SIA32 Cosmic path.
- [ ] Fuzz parsing/validation on the host and replay minimized inputs in Cosmic.
- [ ] Record a compatibility matrix; explicitly mark floats, SIMD, memory64, and WASI unsupported
  until tested on the entire SIA path.
- [ ] Establish stable module ABI-version negotiation and an upgrade policy.

**Exit:** resource limits, hostile inputs, and module cleanup are continuously tested.

## Phase 5 — Optional host-side SIA32 AOT compiler

This is a compilation tool for the development host. It is **not** a Cosmic in-kernel JIT.

- [ ] Define a shared Wasm subset and capability ABI for interpreter and AOT modules.
- [ ] Decide whether to reuse Wasmtime translation components or introduce a small dedicated
  Wasm-to-Cranelift frontend; keep `std` dependencies on the host only.
- [ ] Lower integer-only Wasm operations to Cranelift IR and emit SIA32 relocatable objects
  through the existing Cranelift SIA backend.
- [ ] Define an AOT module container with code, relocations, stack/data requirements,
  import stubs, manifest hash, and ABI version.
- [ ] Implement Cosmic loader relocation, W^X/executable mapping policy, and capability-call trampolines.
- [ ] Prove byte-identical observable behavior against the Wasmi interpreter for a shared test corpus.
- [ ] Add fallback: unsupported instructions/features always run in the interpreter or are rejected
  during packaging—never silently miscompile.
- [ ] Add signed/hashed AOT artifact verification before loading.

**Exit:** one integer-only module is compiled on the host to SIA32, loaded by Cosmic,
and differentially passes the same acceptance cases as its interpreted form.

## Milestone order

1. **W0:** SIA32 `no_std` compile proof.
2. **W1:** capability-gated Wasmi executes in a Cosmic harness.
3. **W2:** real boot-path hello module, trap, and fuel proof.
4. **W3:** hostile-input/resource hardening.
5. **W4:** host-side Cranelift SIA32 AOT proof with interpreter equivalence.

Do not begin W4 until W2 is green on an exact commit.
