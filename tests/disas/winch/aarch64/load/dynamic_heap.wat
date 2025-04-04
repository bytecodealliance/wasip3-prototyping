;;! target = "aarch64"
;;! test = "winch"
;;! flags = "-O static-memory-maximum-size=100 -O dynamic-memory-guard-size=0xffff"

(module
  (memory (export "memory") 17)
  (func (export "run") (param i32) (result i32 i32 i32)
    ;; Within the guard region.
    local.get 0
    i32.load offset=0
    ;; Also within the guard region, bounds check should GVN with previous.
    local.get 0
    i32.load offset=4

    ;; Outside the guard region, needs additional bounds checks.
    local.get 0
    i32.load offset=0x000fffff
  )
  (data (i32.const 0) "\45\00\00\00\a4\01\00\00")
  (data (i32.const 0x000fffff) "\39\05\00\00")
)
