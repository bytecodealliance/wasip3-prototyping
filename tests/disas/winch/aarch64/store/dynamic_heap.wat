;;! target = "aarch64"
;;! test = "winch"
;;! flags = "-O static-memory-maximum-size=0 -O dynamic-memory-guard-size=0xffff"

(module
  (memory (export "memory") 1)
  (func (export "run") (param i32 i32 i32 i32)
    ;; Within the guard region.
    local.get 0
    local.get 1
    i32.store offset=0
    ;; Also within the guard region, bounds check should GVN with previous.
    local.get 0
    local.get 2
    i32.store offset=4
    ;; Outside the guard region, needs additional bounds checks.
    local.get 0
    local.get 3
    i32.store offset=0x000fffff
  )
)
