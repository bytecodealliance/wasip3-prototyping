;;! component_model_async = true

;; Create a future, start a read, close the write end, and cancel the read.
(component
  (type $f (future))
  (core func $new (canon future.new $f))
  (core module $libc (memory (export "mem") 1))
  (core instance $libc (instantiate $libc))
  (core func $read (canon future.read $f async (memory $libc "mem")))
  (core func $cancel (canon future.cancel-read $f))
  (core func $write (canon future.write $f async (memory $libc "mem")))
  (core func $close-write (canon future.close-writable $f))
  (core module $m
    (import "" "new" (func $new (result i64)))
    (import "" "read" (func $read (param i32 i32) (result i32)))
    (import "" "cancel" (func $cancel (param i32) (result i32)))
    (import "" "write" (func $write (param i32 i32) (result i32)))
    (import "" "close-write" (func $close-write (param i32)))

    (func (export "f") (result i32)
      (local $read i32)
      (local $write i32)
      (local $new i64)

      (local.set $new (call $new))
      (local.set $read (i32.wrap_i64 (local.get $new)))
      (local.set $write (i32.wrap_i64 (i64.shr_u (local.get $new) (i64.const 32))))

      ;; start a read
      local.get $read
      i32.const 0
      call $read
      i32.const -1
      i32.ne
      if unreachable end

      ;; close the write end
      local.get $write
      i32.const 0
      call $write
      drop
      local.get $write
      call $close-write

      ;; cancel the read, returning the result
      local.get $read
      call $cancel
    )
  )

  (core instance $i (instantiate $m
    (with "" (instance
      (export "new" (func $new))
      (export "read" (func $read))
      (export "cancel" (func $cancel))
      (export "write" (func $write))
      (export "close-write" (func $close-write))
    ))
  ))

  (func (export "f") (result u32) (canon lift (core func $i "f")))
)

(assert_return (invoke "f") (u32.const 0x11)) ;; expect CLOSED status (not CANCELLED)
