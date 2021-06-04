use std::{
    ffi::CString,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::ruby_sys::{
    ruby_cleanup, ruby_exec_node, ruby_executable_node, ruby_options, ruby_setup,
};

pub struct Cleanup();

impl Drop for Cleanup {
    fn drop(&mut self) {
        unsafe {
            ruby_cleanup(0);
        }
    }
}

/// # Safety
///
/// Must be called in `main()`, or at least a function higher up the stack than
/// any code calling Ruby. Must not drop Cleanup until the very end of the
/// process, after all Ruby execution has finished.
///
/// # Panics
///
/// Panics if called more than once.
///
/// # Examples
///
/// ```no_run
/// let _cleanup = unsafe { magnus::embed::init() };
/// ```
#[inline(always)]
pub unsafe fn init() -> Cleanup {
    init_options(&["-e", ""])
}

#[inline(always)]
unsafe fn init_options(opts: &[&str]) -> Cleanup {
    static INIT: AtomicBool = AtomicBool::new(false);
    match INIT.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(false) => {
            if ruby_setup() != 0 {
                panic!("Failed to setup Ruby");
            };
            let cleanup = Cleanup();
            let mut argv = vec![CString::new("ruby").unwrap()];
            argv.extend(opts.iter().map(|s| CString::new(*s).unwrap()));
            let mut argv = argv
                .iter()
                .map(|cs| cs.as_ptr() as *mut _)
                .collect::<Vec<_>>();
            let node = ruby_options(3, argv.as_mut_ptr());
            let mut status = 0;
            if ruby_executable_node(node, &mut status as *mut _) == 0 {
                panic!("Ruby init code not executable");
            }
            if ruby_exec_node(node) != 0 {
                panic!("Ruby init code failed");
            };
            cleanup
        }
        Err(true) => panic!("Ruby already initialized"),
        r => panic!("unexpected INIT state {:?}", r),
    }
}