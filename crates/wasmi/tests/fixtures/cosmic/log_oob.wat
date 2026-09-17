(module
  (import "cosmic:sys" "log_write"
    (func $log_write (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "_start") (result i32)
    (call $log_write (i32.const 0) (i32.const 65535) (i32.const 2))))
