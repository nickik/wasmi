//! Host conformance vectors for the proposed `cosmic-wasm-v1` boundary.
//!
//! These execute on the host today. They deliberately model only the narrow
//! capability ABI so the same Wasm modules can become Cosmic SIA acceptance
//! vectors once the freestanding Rust target and image path exist.

use sha2::{Digest, Sha256};
use wasmparser::{Parser, Payload};
use wasmi::{Caller, Config, Engine, Extern, Linker, Module, Store, TrapCode};

const OK: i32 = 0;
const EFAULT: i32 = -14;
const EPERM: i32 = -1;
const E2BIG: i32 = -7;
const MAX_LOG_WRITE_BYTES: u32 = 4096;
const MAX_MODULE_ID_BYTES: usize = 64;
const LOG_OK: &[u8] = include_bytes!("../fixtures/cosmic/log_ok.wasm");
const LOG_OOB: &[u8] = include_bytes!("../fixtures/cosmic/log_oob.wasm");
const LOG_DENIED: &[u8] = include_bytes!("../fixtures/cosmic/log_denied.wasm");
const FUEL_LOOP: &[u8] = include_bytes!("../fixtures/cosmic/fuel_loop.wasm");
const INTEGER_CONTROL: &[u8] = include_bytes!("../fixtures/cosmic/integer_control.wasm");
const MEMORY_UNBOUNDED: &[u8] = include_bytes!("../fixtures/cosmic/memory_unbounded.wasm");

#[derive(Default)]
struct CosmicHost {
    logs: Vec<Vec<u8>>,
}

const COSMIC_WASM_V1: &str = "cosmic-wasm-v1";

#[derive(Clone, Copy)]
struct Manifest {
    module_id: &'static str,
    abi: &'static str,
    sha256: [u8; 32],
    max_module_bytes: usize,
    max_imports: u32,
    max_memory_pages: u64,
    fuel: u64,
    logging_capability: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum LoadError {
    InvalidModuleId,
    AbiMismatch,
    HashMismatch,
    ModuleTooLarge,
    TooManyImports,
    MemoryLimitExceeded,
    UnknownImport,
    CapabilityDenied,
    InvalidModule,
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn manifest(module_id: &'static str, bytes: &[u8]) -> Manifest {
    Manifest {
        module_id,
        abi: COSMIC_WASM_V1,
        sha256: sha256(bytes),
        max_module_bytes: bytes.len(),
        max_imports: 1,
        max_memory_pages: 1,
        fuel: 1_000,
        logging_capability: true,
    }
}

fn validate_manifest(bytes: &[u8], manifest: Manifest) -> Result<(), LoadError> {
    if manifest.module_id.is_empty() || manifest.module_id.len() > MAX_MODULE_ID_BYTES {
        return Err(LoadError::InvalidModuleId);
    }
    if manifest.abi != COSMIC_WASM_V1 {
        return Err(LoadError::AbiMismatch);
    }
    if sha256(bytes) != manifest.sha256 {
        return Err(LoadError::HashMismatch);
    }
    if bytes.len() > manifest.max_module_bytes {
        return Err(LoadError::ModuleTooLarge);
    }
    let mut imports = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(|_| LoadError::InvalidModule)? {
            Payload::ImportSection(section) => {
                imports += section.count();
                for import in section {
                    let import = import.map_err(|_| LoadError::InvalidModule)?;
                    if import.module != "cosmic:sys" || import.name != "log_write" {
                        return Err(LoadError::UnknownImport);
                    }
                    if !manifest.logging_capability {
                        return Err(LoadError::CapabilityDenied);
                    }
                }
            }
            Payload::MemorySection(section) => {
                for memory in section {
                    let memory = memory.map_err(|_| LoadError::InvalidModule)?;
                    if memory.initial > manifest.max_memory_pages
                        || memory.maximum.unwrap_or(u64::MAX) > manifest.max_memory_pages
                    {
                        return Err(LoadError::MemoryLimitExceeded);
                    }
                }
            }
            _ => {}
        }
    }
    if imports > manifest.max_imports {
        return Err(LoadError::TooManyImports);
    }
    Ok(())
}

fn log_write(mut caller: Caller<CosmicHost>, cap: i32, ptr: i32, len: i32) -> i32 {
    if cap != 0 {
        return EPERM;
    }
    let len = len as u32;
    if len > MAX_LOG_WRITE_BYTES {
        return E2BIG;
    }
    let Some(Extern::Memory(memory)) = caller.get_export("memory") else {
        return EFAULT;
    };
    let mut bytes = vec![0; len as usize];
    if memory.read(&caller, ptr as u32 as usize, &mut bytes).is_err() {
        return EFAULT;
    }
    caller.data_mut().logs.push(bytes);
    OK
}

fn test_setup() -> (Store<CosmicHost>, Linker<CosmicHost>) {
    let mut config = Config::default();
    config.consume_fuel(true);
    config.compilation_mode(wasmi::CompilationMode::Eager);
    let engine = Engine::new(&config);
    let mut store = Store::new(&engine, CosmicHost::default());
    store.set_fuel(1_000).unwrap();
    let mut linker = Linker::new(&engine);
    linker
        .func_wrap("cosmic:sys", "log_write", log_write)
        .unwrap();
    (store, linker)
}

#[test]
fn cosmic_log_write_copies_guest_memory_through_capability() {
    let (mut store, linker) = test_setup();
    let module = Module::new(store.engine(), LOG_OK).unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let start = instance
        .get_typed_func::<(), i32>(&store, "_start")
        .unwrap();

    assert_eq!(start.call(&mut store, ()).unwrap(), OK);
    assert_eq!(store.data().logs, [b"hello cosmic".to_vec()]);
}

#[test]
fn cosmic_log_write_rejects_out_of_bounds_guest_memory_without_logging() {
    let (mut store, linker) = test_setup();
    let module = Module::new(store.engine(), LOG_OOB).unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let start = instance
        .get_typed_func::<(), i32>(&store, "_start")
        .unwrap();

    assert_eq!(start.call(&mut store, ()).unwrap(), EFAULT);
    assert!(store.data().logs.is_empty());
}

#[test]
fn cosmic_log_write_rejects_an_undelegated_capability_without_logging() {
    let (mut store, linker) = test_setup();
    let module = Module::new(store.engine(), LOG_DENIED).unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let start = instance
        .get_typed_func::<(), i32>(&store, "_start")
        .unwrap();

    assert_eq!(start.call(&mut store, ()).unwrap(), EPERM);
    assert!(store.data().logs.is_empty());
}

#[test]
fn cosmic_runtime_reports_fuel_exhaustion_as_a_guest_failure() {
    let (mut store, linker) = test_setup();
    let module = Module::new(store.engine(), FUEL_LOOP).unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let start = instance.get_typed_func::<(), ()>(&store, "_start").unwrap();
    store.set_fuel(10).unwrap();

    let error = start.call(&mut store, ()).unwrap_err();
    assert_eq!(error.as_trap_code(), Some(TrapCode::OutOfFuel));
    assert!(store.get_fuel().unwrap() < 10);
}

#[test]
fn cosmic_integer_control_fixture_runs_without_imports() {
    let (mut store, linker) = test_setup();
    let module = Module::new(store.engine(), INTEGER_CONTROL).unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let start = instance
        .get_typed_func::<(), i32>(&store, "_start")
        .unwrap();

    assert_eq!(start.call(&mut store, ()).unwrap(), 3);
}

#[test]
#[cfg_attr(not(feature = "wat"), ignore)]
fn cosmic_wat_sources_reproduce_checked_in_binary_fixtures() {
    for (source, fixture) in [
        (include_str!("../fixtures/cosmic/log_ok.wat"), LOG_OK),
        (include_str!("../fixtures/cosmic/log_oob.wat"), LOG_OOB),
        (include_str!("../fixtures/cosmic/log_denied.wat"), LOG_DENIED),
        (include_str!("../fixtures/cosmic/fuel_loop.wat"), FUEL_LOOP),
        (
            include_str!("../fixtures/cosmic/integer_control.wat"),
            INTEGER_CONTROL,
        ),
        (
            include_str!("../fixtures/cosmic/memory_unbounded.wat"),
            MEMORY_UNBOUNDED,
        ),
    ] {
        assert_eq!(wat::parse_str(source).unwrap(), fixture);
    }
}

#[test]
fn cosmic_manifest_accepts_the_checked_in_fixture_profile() {
    let log_manifest = manifest("log-ok", LOG_OK);
    assert_eq!(log_manifest.module_id, "log-ok");
    assert!(log_manifest.logging_capability);
    assert_eq!(log_manifest.fuel, 1_000);
    assert_eq!(validate_manifest(LOG_OK, log_manifest), Ok(()));

    let mut no_imports = manifest("integer-control", INTEGER_CONTROL);
    no_imports.max_imports = 0;
    no_imports.logging_capability = false;
    assert_eq!(validate_manifest(INTEGER_CONTROL, no_imports), Ok(()));
}

#[test]
fn cosmic_manifest_rejects_hash_abi_and_resource_limit_failures_before_execution() {
    let manifest = manifest("log-ok", LOG_OK);

    let mut missing_module_id = manifest;
    missing_module_id.module_id = "";
    assert_eq!(
        validate_manifest(LOG_OK, missing_module_id),
        Err(LoadError::InvalidModuleId)
    );

    let mut changed = LOG_OK.to_vec();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    assert_eq!(validate_manifest(&changed, manifest), Err(LoadError::HashMismatch));

    let mut wrong_abi = manifest;
    wrong_abi.abi = "cosmic-wasm-v0";
    assert_eq!(validate_manifest(LOG_OK, wrong_abi), Err(LoadError::AbiMismatch));

    let mut small_module = manifest;
    small_module.max_module_bytes -= 1;
    assert_eq!(validate_manifest(LOG_OK, small_module), Err(LoadError::ModuleTooLarge));

    let mut no_imports = manifest;
    no_imports.max_imports = 0;
    assert_eq!(validate_manifest(LOG_OK, no_imports), Err(LoadError::TooManyImports));

    let mut no_memory = manifest;
    no_memory.max_memory_pages = 0;
    assert_eq!(validate_manifest(LOG_OK, no_memory), Err(LoadError::MemoryLimitExceeded));
}

#[test]
fn cosmic_manifest_rejects_malformed_payloads_before_execution() {
    let bytes = b"not a wasm module";
    let manifest = manifest("malformed", bytes);
    assert_eq!(validate_manifest(bytes, manifest), Err(LoadError::InvalidModule));
}

#[test]
fn cosmic_manifest_rejects_unbounded_linear_memory_before_execution() {
    let manifest = manifest("memory-unbounded", MEMORY_UNBOUNDED);
    assert_eq!(
        validate_manifest(MEMORY_UNBOUNDED, manifest),
        Err(LoadError::MemoryLimitExceeded)
    );
}

#[test]
fn cosmic_loader_rejects_unknown_and_incompatible_imports_before_guest_execution() {
    let mut unknown = LOG_OK.to_vec();
    let name = unknown
        .windows(b"log_write".len())
        .position(|window| window == b"log_write")
        .unwrap();
    unknown[name..name + b"log_write".len()].copy_from_slice(b"log_wrong");
    let unknown_manifest = manifest("unknown-import", &unknown);
    assert_eq!(
        validate_manifest(&unknown, unknown_manifest),
        Err(LoadError::UnknownImport)
    );
    let (mut store, linker) = test_setup();
    let unknown_module = Module::new(store.engine(), unknown).unwrap();
    assert!(linker.instantiate_and_start(&mut store, &unknown_module).is_err());
    assert!(store.data().logs.is_empty());

    let mut config = Config::default();
    config.consume_fuel(true);
    config.compilation_mode(wasmi::CompilationMode::Eager);
    let engine = Engine::new(&config);
    let mut store = Store::new(&engine, CosmicHost::default());
    store.set_fuel(1_000).unwrap();
    let mut linker = Linker::new(&engine);
    linker
        .func_wrap("cosmic:sys", "log_write", || {})
        .unwrap();
    let incompatible_module = Module::new(store.engine(), LOG_OK).unwrap();
    assert!(linker
        .instantiate_and_start(&mut store, &incompatible_module)
        .is_err());
    assert!(store.data().logs.is_empty());
}

#[test]
fn cosmic_manifest_rejects_requested_imports_without_the_capability() {
    let mut manifest = manifest("log-without-capability", LOG_OK);
    manifest.logging_capability = false;
    assert_eq!(
        validate_manifest(LOG_OK, manifest),
        Err(LoadError::CapabilityDenied)
    );
}

#[test]
fn cosmic_fixture_lifecycle_releases_host_state_between_runs() {
    for _ in 0..2 {
        let (mut store, linker) = test_setup();
        let module = Module::new(store.engine(), LOG_OK).unwrap();
        let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
        let start = instance
            .get_typed_func::<(), i32>(&store, "_start")
            .unwrap();
        assert_eq!(start.call(&mut store, ()).unwrap(), OK);
        assert_eq!(store.data().logs, [b"hello cosmic".to_vec()]);
    }
}
