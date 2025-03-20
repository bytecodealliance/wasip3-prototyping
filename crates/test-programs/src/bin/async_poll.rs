mod bindings {
    wit_bindgen::generate!({
        path: "../misc/component-async-tests/wit",
        world: "poll",
    });

    use super::Component;
    export!(Component);
}

use bindings::{exports::local::local::run::Guest, local::local::ready};

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[waitable-set-new]"]
    fn waitable_set_new() -> u32;
}
#[cfg(not(target_arch = "wasm32"))]
unsafe fn waitable_set_new() -> u32 {
    unreachable!()
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[waitable-join]"]
    fn waitable_join(waitable: u32, set: u32);
}
#[cfg(not(target_arch = "wasm32"))]
unsafe fn waitable_join(_: u32, _: u32) {
    unreachable!()
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "$root")]
unsafe extern "C" {
    #[link_name = "[waitable-set-drop]"]
    fn waitable_set_drop(set: u32);
}
#[cfg(not(target_arch = "wasm32"))]
unsafe fn waitable_set_drop(_: u32) {
    unreachable!()
}

fn waitable_set_poll(set: u32) -> Option<(u32, u32, u32)> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        _ = set;
        unreachable!();
    }

    #[cfg(target_arch = "wasm32")]
    {
        #[link(wasm_import_module = "$root")]
        unsafe extern "C" {
            #[link_name = "[waitable-set-poll]"]
            fn poll(_: u32, _: *mut u32) -> u32;
        }
        let mut payload = [0u32; 3];
        if unsafe { poll(set, payload.as_mut_ptr()) } != 0 {
            Some((payload[0], payload[1], payload[2]))
        } else {
            None
        }
    }
}

fn async_when_ready() -> u32 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        unreachable!()
    }

    #[cfg(target_arch = "wasm32")]
    {
        #[link(wasm_import_module = "local:local/ready")]
        unsafe extern "C" {
            #[link_name = "[async-lower]when-ready"]
            fn call_when_ready(_: *mut u8, _: *mut u8) -> u32;
        }
        unsafe { call_when_ready(std::ptr::null_mut(), std::ptr::null_mut()) }
    }
}

/// Call the `subtask.drop` canonical built-in function.
fn subtask_drop(subtask: u32) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        _ = subtask;
        unreachable!();
    }

    #[cfg(target_arch = "wasm32")]
    {
        #[link(wasm_import_module = "$root")]
        unsafe extern "C" {
            #[link_name = "[subtask-drop]"]
            fn subtask_drop(_: u32);
        }
        unsafe {
            subtask_drop(subtask);
        }
    }
}

const STATUS_RETURNED: u32 = 3;

const EVENT_CALL_RETURNED: u32 = 3;

struct Component;

impl Guest for Component {
    fn run() {
        unsafe {
            ready::set_ready(false);

            let set = waitable_set_new();

            assert!(waitable_set_poll(set).is_none());

            let result = async_when_ready();
            let status = result >> 30;
            let call = result & !(0b11 << 30);
            assert!(status != STATUS_RETURNED);
            waitable_join(call, set);

            assert!(waitable_set_poll(set).is_none());

            ready::set_ready(true);

            let Some((EVENT_CALL_RETURNED, task, _)) = waitable_set_poll(set) else {
                panic!()
            };

            subtask_drop(task);

            assert!(waitable_set_poll(set).is_none());

            assert!(async_when_ready() == STATUS_RETURNED << 30);

            assert!(waitable_set_poll(set).is_none());

            waitable_set_drop(set);
        }
    }
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
