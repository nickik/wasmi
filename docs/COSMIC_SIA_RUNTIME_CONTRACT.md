# Proposed Cosmic Wasmi Runtime Contract (W0)

**Status:** PROPOSED — requires matching acceptance in `nickik/Cosmic` before it is frozen.

## 1. Placement and authority

`wasmi` runs as an **unprivileged Cosmic task** that hosts one or more WebAssembly modules.
It is not part of the privileged Forge kernel and it does not implement Cosmic capability,
memory-pool, scheduling, or task-lifecycle policy. The Forge kernel remains the sole authority
for those policies.

The runtime receives only handles explicitly delegated to its hosting task. A guest module never
receives a raw Cosmic handle, kernel pointer, physical address, page-table detail, or syscall
number. Every guest-visible operation is a versioned `cosmic:sys` import.

## 2. Initial module profile

The initial accepted profile is `cosmic-wasm-v1`:

- Wasm core integer execution only (`i32` and `i64`); no floating point or SIMD.
- One defined linear memory, maximum configured by the host manifest.
- No WASI, filesystem, sockets, clock, random, threads, shared memory, exceptions, GC,
  component model, dynamic linking, or `memory64`.
- Module start is an exported `() -> ()` function named `_start`.
- Imports must be declared under module name `cosmic:sys`; unknown modules, names, or signatures
  fail validation/instantiation before guest code runs.

`i64` is part of the Wasm profile because Wasmi requires it internally even when a guest is kept
integer-only. SIA execution therefore depends on complete generated-Rust I64 support.

## 3. ABI version and imports

The manifest declares `abi = "cosmic-wasm-v1"`. The initial import set is intentionally small:

```wasm
(import "cosmic:sys" "log_write"
  (func (param i32 i32 i32) (result i32)))
(import "cosmic:sys" "yield"
  (func (result i32)))
```

`log_write(cap, ptr, len) -> status`

- `cap` is a guest-local opaque index, resolved by the runtime against the manifest's delegated
  logging capability. It is not a Cosmic handle.
- `ptr` and `len` address guest linear memory. The runtime uses checked addition and bounds
  validation before copying any bytes.
- The host copies at most `max_log_write_bytes` bytes into a host-owned buffer before logging.
- The initial text policy is UTF-8 required; invalid UTF-8 returns `EINVAL` without partial log
  emission.

`yield() -> status` asks the runtime to return control to its Cosmic scheduler boundary. It
does not schedule directly and does not carry authority.

Statuses are stable signed 32-bit values: `0` success; nonzero values are published contract
errors. A future version may add imports but must use a new ABI version or an explicitly
versioned optional capability set.

## 4. Resource accounting and limits

The manifest fixes, and the runtime reports, at least:

| Resource | Enforcement point |
| --- | --- |
| module byte length and import count | before translation |
| translated bytecode and runtime metadata | before publication |
| linear-memory minimum/maximum pages | instantiation and grow |
| tables, globals, call depth, instances | instantiation/execution |
| fuel | each Wasmi execution quantum |
| host-call byte count and log-write length | every import |

Every runtime allocation is charged to a host-selected Cosmic `MemoryPool` through the task's
normal authority path. The runtime cannot select another pool. Resource denial must leave no
partially published guest instance or capability state.

## 5. Lifecycle and faults

The lifecycle is:

`load -> hash/manifest check -> validate -> reserve resources -> instantiate -> run -> destroy`.

Validation failure, instantiation failure, a Wasm trap, fuel exhaustion, an invalid host-call
pointer/length, or revocation of the hosting task's capability ends that module only. The runtime
emits a deterministic diagnostic containing module ID, ABI version, result class, consumed fuel,
and final linear-memory page count. It must release the guest instance, translated data,
capability-local state, and all resource charges on every terminal path.

The initial stable result classes are `Returned`, `Trapped`, `FuelExhausted`,
`ValidationFailed`, `InstantiationFailed`, `CapabilityDenied`, and `HostCallFailed`.

## 6. Packaging

The eventual Cosmic image/object-store entry has:

```text
module.wasm
manifest (module ID, content hash, abi, limits, requested capabilities)
optional AOT payload (later only)
```

The hash covers the Wasm payload and canonical manifest. The first native image format is
intentionally deferred to Cosmic M27--M29. The contract does not expose CSM1, SIA relocation,
or loader-internal details as a Wasm ABI.

## 7. Acceptance vectors

The shared vector set must include:

1. `log_write` success and normal return;
2. valid integer control flow and linear-memory reads/writes;
3. out-of-bounds memory trap;
4. fuel exhaustion;
5. denied or signature-mismatched import;
6. malformed module rejection before instantiation;
7. repeated load/run/destroy with resource accounting restored.

The same vectors run first on a host Wasmi harness and later through the real
`ROM -> MMU -> Cosmic -> runtime task` Lighting route. Host success is not native-SIA acceptance.

## 8. Deferred AOT path

A host-side Wasm-to-Cranelift-to-SIA32 compiler may create a signed optional AOT payload only
after the interpreted path is green. It must share this ABI, retain interpreter fallback for
unsupported modules, and demonstrate equivalent observable results on the shared vectors. It is
never an in-kernel JIT.
