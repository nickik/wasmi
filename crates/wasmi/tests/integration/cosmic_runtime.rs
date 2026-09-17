//! Host conformance vectors for the proposed `cosmic-wasm-v1` boundary.
//!
//! These execute on the host today. They deliberately model only the narrow
//! capability ABI so the same Wasm modules can become Cosmic SIA acceptance
//! vectors once the freestanding Rust target and image path exist.

use wasmi::{Caller, Config, Engine, Extern, Linker, Module, Store};

const OK: i32 = 0;
const EFAULT: i32 = -14;
const EPERM: i32 = -1;
const E2BIG: i32 = -7;
const MAX_LOG_WRITE_BYTES: u32 = 4096;

#[derive(Default)]
struct CosmicHost {
    logs: Vec<Vec<u8>>,
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
#[cfg_attr(not(feature = "wat"), ignore)]
fn cosmic_log_write_copies_guest_memory_through_capability() {
    let (mut store, linker) = test_setup();
    let module = Module::new(
        store.engine(),
        r#"
            (module
                (import "cosmic:sys" "log_write"
                    (func $log_write (param i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 8) "hello cosmic")
                (func (export "_start") (result i32)
                    (call $log_write (i32.const 0) (i32.const 8) (i32.const 12)))
            )
        "#,
    )
    .unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let start = instance
        .get_typed_func::<(), i32>(&store, "_start")
        .unwrap();

    assert_eq!(start.call(&mut store, ()).unwrap(), OK);
    assert_eq!(store.data().logs, [b"hello cosmic".to_vec()]);
}

#[test]
#[cfg_attr(not(feature = "wat"), ignore)]
fn cosmic_log_write_rejects_out_of_bounds_guest_memory_without_logging() {
    let (mut store, linker) = test_setup();
    let module = Module::new(
        store.engine(),
        r#"
            (module
                (import "cosmic:sys" "log_write"
                    (func $log_write (param i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (func (export "_start") (result i32)
                    (call $log_write (i32.const 0) (i32.const 65535) (i32.const 2)))
            )
        "#,
    )
    .unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let start = instance
        .get_typed_func::<(), i32>(&store, "_start")
        .unwrap();

    assert_eq!(start.call(&mut store, ()).unwrap(), EFAULT);
    assert!(store.data().logs.is_empty());
}

#[test]
#[cfg_attr(not(feature = "wat"), ignore)]
fn cosmic_log_write_rejects_an_undelegated_capability_without_logging() {
    let (mut store, linker) = test_setup();
    let module = Module::new(
        store.engine(),
        r#"
            (module
                (import "cosmic:sys" "log_write"
                    (func $log_write (param i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 0) "x")
                (func (export "_start") (result i32)
                    (call $log_write (i32.const 1) (i32.const 0) (i32.const 1)))
            )
        "#,
    )
    .unwrap();
    let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
    let start = instance
        .get_typed_func::<(), i32>(&store, "_start")
        .unwrap();

    assert_eq!(start.call(&mut store, ()).unwrap(), EPERM);
    assert!(store.data().logs.is_empty());
}
