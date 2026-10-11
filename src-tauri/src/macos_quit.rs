//! Convert macOS Dock Quit and Cmd+Q into the same exit path as the menu-bar item.
//!
//! Tao does not implement `applicationShouldTerminate:`. Dock Quit and Cmd+Q
//! therefore call `NSApp.terminate`, skip `RunEvent::ExitRequested`, and kill
//! the control process in `applicationWillTerminate`. The gateway sidecar and
//! isolated instances are left running. This hook cancels the native terminate
//! and posts `app.exit(0)` so the existing cleanup can run.

use std::ffi::c_char;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::sel;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

unsafe extern "C-unwind" fn application_should_terminate(
    _this: *mut AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> usize {
    if let Some(app) = APP.get() {
        app.exit(0);
    }
    NSApplicationTerminateReply::TerminateCancel.0
}

/// Install once on the existing Tao application delegate.
pub fn install(app: AppHandle) {
    let _ = APP.set(app);
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        INSTALLED.store(false, Ordering::SeqCst);
        eprintln!("R Codex macOS quit hook skipped: not on the main thread");
        return;
    };
    let ns_app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = ns_app.delegate() else {
        INSTALLED.store(false, Ordering::SeqCst);
        eprintln!("R Codex macOS quit hook skipped: NSApp has no delegate yet");
        return;
    };
    let obj: &AnyObject = delegate.as_ref();
    let cls: &AnyClass = obj.class();
    let sel = sel!(applicationShouldTerminate:);
    // NSUInteger return, id self, SEL _cmd, id sender
    let types: *const c_char = c"Q@:@".as_ptr();
    let imp = unsafe {
        std::mem::transmute::<
            unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
            objc2::runtime::Imp,
        >(application_should_terminate)
    };
    let added = unsafe {
        objc2::ffi::class_addMethod(cls as *const AnyClass as *mut AnyClass, sel, imp, types)
    };
    if added.as_bool() {
        return;
    }
    let method = unsafe { objc2::ffi::class_getInstanceMethod(cls, sel) };
    if method.is_null() {
        INSTALLED.store(false, Ordering::SeqCst);
        eprintln!("R Codex macOS quit hook skipped: could not add applicationShouldTerminate:");
        return;
    }
    let _ = unsafe { objc2::ffi::method_setImplementation(method, imp) };
}
