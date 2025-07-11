;;! target = "aarch64"
;;! test = "compile"
;;! flags = " -C cranelift-enable-heap-access-spectre-mitigation -O static-memory-maximum-size=0 -O static-memory-guard-size=0 -O dynamic-memory-guard-size=0"

;; !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
;; !!! GENERATED BY 'make-load-store-tests.sh' DO NOT EDIT !!!
;; !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

(module
  (memory i32 1)

  (func (export "do_store") (param i32 i32)
    local.get 0
    local.get 1
    i32.store offset=0xffff0000)

  (func (export "do_load") (param i32) (result i32)
    local.get 0
    i32.load offset=0xffff0000))

;; wasm[0]::function[0]:
;;       stp     x29, x30, [sp, #-0x10]!
;;       mov     x29, sp
;;       mov     w13, w4
;;       mov     w14, #-0xfffc
;;       adds    x13, x13, x14
;;       b.hs    #0x48
;;   18: ldr     x14, [x2, #0x40]
;;       ldr     x0, [x2, #0x38]
;;       mov     x15, #0
;;       add     x0, x0, w4, uxtw
;;       mov     x1, #0xffff0000
;;       add     x0, x0, x1
;;       cmp     x13, x14
;;       csel    x15, x15, x0, hi
;;       csdb
;;       str     w5, [x15]
;;       ldp     x29, x30, [sp], #0x10
;;       ret
;;   48: .byte   0x1f, 0xc1, 0x00, 0x00
;;
;; wasm[0]::function[1]:
;;       stp     x29, x30, [sp, #-0x10]!
;;       mov     x29, sp
;;       mov     w13, w4
;;       mov     w14, #-0xfffc
;;       adds    x13, x13, x14
;;       b.hs    #0xa8
;;   78: ldr     x14, [x2, #0x40]
;;       ldr     x0, [x2, #0x38]
;;       mov     x15, #0
;;       add     x0, x0, w4, uxtw
;;       mov     x1, #0xffff0000
;;       add     x0, x0, x1
;;       cmp     x13, x14
;;       csel    x15, x15, x0, hi
;;       csdb
;;       ldr     w2, [x15]
;;       ldp     x29, x30, [sp], #0x10
;;       ret
;;   a8: .byte   0x1f, 0xc1, 0x00, 0x00
