(module
  (func (export "_start") (result i32)
    (local $value i32)
    (local.set $value (i32.const 0))
    (block $done
      (loop $count
        (br_if $done (i32.ge_u (local.get $value) (i32.const 3)))
        (local.set $value (i32.add (local.get $value) (i32.const 1)))
        (br $count)))
    (local.get $value)))
