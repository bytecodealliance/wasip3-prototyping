mod bindings {
    wit_bindgen::generate!({
        path: "../misc/component-async-tests/wit",
        world: "poll",
    });

    use super::Component;
    export!(Component);
}

use {
    bindings::{exports::local::local::run::Guest, local::local::ready},
    test_programs::async_::{
        subtask_drop, waitable_join, waitable_set_drop, waitable_set_new, waitable_set_poll,
        EVENT_CALL_RETURNED, STATUS_RETURNED,
    },
};

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

struct Component;

impl Guest for Component {
    fn run() {
        unsafe {
            ready::set_ready(false);

            let set = waitable_set_new();

            assert!(waitable_set_poll(set).is_none());

            let result = async_when_ready();
            let status = result & 0xf;
            let call = result >> 4;
            assert!(status != STATUS_RETURNED);
            waitable_join(call, set);

            assert!(waitable_set_poll(set).is_none());

            ready::set_ready(true);

            let Some((EVENT_CALL_RETURNED, task, _)) = waitable_set_poll(set) else {
                panic!()
            };

            assert_eq!(call, task);

            subtask_drop(task);

            assert!(waitable_set_poll(set).is_none());

            assert!(async_when_ready() == STATUS_RETURNED);

            assert!(waitable_set_poll(set).is_none());

            waitable_set_drop(set);
        }
    }
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
