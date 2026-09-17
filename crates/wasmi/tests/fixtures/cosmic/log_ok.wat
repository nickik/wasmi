(module
  (import "cosmic:sys" "log_write"
    (func $log_write (param i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) "hello cosmic")
  (func (export "_start") (result i32)
    (call $log_write (i32.const 0) (i32.const 8) (i32.const 12))))
