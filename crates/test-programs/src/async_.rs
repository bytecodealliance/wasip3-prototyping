#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[waitable-set-new]"]
    pub fn waitable_set_new() -> u32;
}
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn waitable_set_new() -> u32 {
    unreachable!()
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[waitable-join]"]
    pub fn waitable_join(waitable: u32, set: u32);
}
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn waitable_join(_: u32, _: u32) {
    unreachable!()
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[waitable-set-drop]"]
    pub fn waitable_set_drop(set: u32);
}
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn waitable_set_drop(_: u32) {
    unreachable!()
}

#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn waitable_set_poll_raw(_: u32, _: *mut u32) -> u32 {
    unreachable!()
}
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[waitable-set-poll]"]
    pub fn waitable_set_poll_raw(_: u32, _: *mut u32) -> u32;
}

pub fn waitable_set_poll(set: u32) -> Option<(u32, u32, u32)> {
    let mut payload = [0u32; 3];
    if unsafe { waitable_set_poll_raw(set, payload.as_mut_ptr()) } != 0 {
        Some((payload[0], payload[1], payload[2]))
    } else {
        None
    }
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[waitable-set-wait]"]
    pub fn waitable_set_wait(set: u32, results: *mut u32) -> u32;
}
#[cfg(not(target_arch = "wasm32"))]
pub unsafe extern "C" fn waitable_set_wait(_set: u32, _results: *mut u32) -> u32 {
    unreachable!()
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[subtask-drop]"]
    pub fn subtask_drop(task: u32);
}
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn subtask_drop(_: u32) {
    unreachable!()
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[context-get-1]"]
    pub fn context_get() -> u32;
}
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn context_get() -> u32 {
    unreachable!()
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[context-set-1]"]
    pub fn context_set(value: u32);
}
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn context_set(_: u32) {
    unreachable!()
}

pub const STATUS_STARTING: u32 = 0;
pub const STATUS_STARTED: u32 = 1;
pub const STATUS_RETURNED: u32 = 2;

pub const EVENT_NONE: u32 = 0;
pub const EVENT_SUBTASK: u32 = 1;

pub const CALLBACK_CODE_EXIT: u32 = 0;
pub const CALLBACK_CODE_YIELD: u32 = 1;
pub const CALLBACK_CODE_WAIT: u32 = 2;
pub const CALLBACK_CODE_POLL: u32 = 3;
